//! Proof-of-concept PyO3 bindings for the faust-rs interpreter (FBC) backend.
//!
//! Exposes a minimal Python API that compiles a Faust `.dsp` source string to
//! FBC bytecode and runs it through the native Rust interpreter:
//!
//! ```python
//! import faust_rs
//! dsp = faust_rs.compile("process = _, _ : + : *(0.5);", sample_rate=48000)
//! outs = dsp.compute([[0.1, 0.2], [0.3, 0.4]])   # channels -> channels
//! ```
//!
//! Scope is deliberately narrow (persistent single-block render) to demonstrate
//! the binding path, not to be a full host API. Both single (`f32`) and double
//! (`f64`) precision are supported via the `double=` flag on `compile`.

use std::io::Cursor;

use codegen::backends::interp::{
    FbcDspFactory, FbcReal, InterpOptions, OwnedFbcDspInstance, read_fbc,
};
use compiler::{Compiler, RealType, SignalFirLane};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Precision-erased owning instance.
///
/// The interpreter is generic over `FbcReal` (`f32` / `f64`); this enum lets a
/// single `Dsp` handle carry either precision, chosen at `compile()` time. It
/// mirrors the FFI's `FbcDspFactoryAny` type-erasure approach.
enum Engine {
    F32(OwnedFbcDspInstance<f32>),
    F64(OwnedFbcDspInstance<f64>),
}

/// A compiled Faust DSP program with a persistent, stateful interpreter
/// instance.
///
/// Wraps [`OwnedFbcDspInstance`], which owns its factory (immutable bytecode)
/// alongside the runtime state, so no self-referential borrowing or `unsafe` is
/// needed here — the safe, no-lifetime owning instance lives in the `codegen`
/// interpreter backend. A single instance is held across calls, so DSP state —
/// recursive filters, oscillator phase, delay lines — carries from one
/// `compute()` to the next. `init()` runs once at construction; call
/// [`Dsp::reset`] to clear state.
///
/// Audio crosses the Python boundary as `f64` (Python's native float):
/// lossless for a double-precision DSP, and cast to/from `f32` for a
/// single-precision one.
#[pyclass]
struct Dsp {
    engine: Engine,
    sample_rate: i32,
    num_inputs: i32,
    num_outputs: i32,
    name: String,
    double: bool,
}

/// Initializes a persistent owning instance over `factory` at `sample_rate`.
fn build_instance<R: FbcReal>(
    factory: FbcDspFactory<R>,
    sample_rate: i32,
) -> OwnedFbcDspInstance<R> {
    let mut instance = OwnedFbcDspInstance::from_factory(factory);
    instance.init(sample_rate);
    instance
}

/// Renders one block through an instance of precision `R`, marshaling audio
/// to/from Python's `f64` buffers. State advances on the instance.
fn render<R: FbcReal>(
    instance: &mut OwnedFbcDspInstance<R>,
    inputs: &[Vec<f64>],
    count: i32,
    num_out: usize,
) -> PyResult<Vec<Vec<f64>>> {
    let in_bufs: Vec<Vec<R>> = inputs
        .iter()
        .map(|ch| ch.iter().map(|&x| R::from_f64(x)).collect())
        .collect();
    let in_refs: Vec<&[R]> = in_bufs.iter().map(Vec::as_slice).collect();

    let mut out_bufs: Vec<Vec<R>> = vec![vec![R::default(); count.max(0) as usize]; num_out];
    let mut out_refs: Vec<&mut [R]> = out_bufs.iter_mut().map(Vec::as_mut_slice).collect();

    instance
        .try_compute(count, &in_refs, &mut out_refs)
        .map_err(|e| PyValueError::new_err(format!("interpreter runtime error: {e}")))?;

    Ok(out_bufs
        .iter()
        .map(|ch| ch.iter().map(|&x| x.to_f64()).collect())
        .collect())
}

impl Dsp {
    /// Wraps a precision-erased engine, caching audio-layout metadata.
    fn new(engine: Engine, sample_rate: i32) -> Self {
        let (num_inputs, num_outputs, name, double) = match &engine {
            Engine::F32(i) => (
                i.get_num_inputs(),
                i.get_num_outputs(),
                i.factory().name.clone(),
                false,
            ),
            Engine::F64(i) => (
                i.get_num_inputs(),
                i.get_num_outputs(),
                i.factory().name.clone(),
                true,
            ),
        };
        Self {
            engine,
            sample_rate,
            num_inputs,
            num_outputs,
            name,
            double,
        }
    }
}

#[pymethods]
impl Dsp {
    /// Number of audio input channels the DSP expects.
    #[getter]
    fn num_inputs(&self) -> i32 {
        self.num_inputs
    }

    /// Number of audio output channels the DSP produces.
    #[getter]
    fn num_outputs(&self) -> i32 {
        self.num_outputs
    }

    /// Render sample rate the instance is initialized with.
    #[getter]
    fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    /// Compiled DSP name.
    #[getter]
    fn name(&self) -> String {
        self.name.clone()
    }

    /// Sample precision of the interpreter: `"double"` (`f64`) or `"float"`
    /// (`f32`).
    #[getter]
    fn precision(&self) -> &'static str {
        if self.double { "double" } else { "float" }
    }

    /// Total blocks rendered by the persistent instance since construction.
    ///
    /// Monotonic: advances with every `compute()`. It is *not* zeroed by
    /// `reset()` (which clears audio DSP state, not this bookkeeping counter),
    /// so a rising `cycle` evidences that one instance is reused across calls.
    #[getter]
    fn cycle(&self) -> usize {
        match &self.engine {
            Engine::F32(i) => i.cycle(),
            Engine::F64(i) => i.cycle(),
        }
    }

    /// Re-initialize the instance, clearing all DSP state (filter memory,
    /// oscillator phase, delay lines) as if freshly compiled.
    fn reset(&mut self) {
        match &mut self.engine {
            Engine::F32(i) => i.init(self.sample_rate),
            Engine::F64(i) => i.init(self.sample_rate),
        }
    }

    /// Render one block of audio, advancing the persistent instance state.
    ///
    /// State carries across successive `compute()` calls (stateful DSPs such as
    /// oscillators and filters continue where the previous block left off).
    ///
    /// `inputs` is a list of input channels (each a list of samples); all
    /// channels must share the same length. For a DSP with zero inputs, pass an
    /// empty list and set `frames`. Returns a list of output channels. Samples
    /// cross as Python floats (`f64`); a `float`-precision DSP casts internally.
    #[pyo3(signature = (inputs, frames = None))]
    fn compute(&mut self, inputs: Vec<Vec<f64>>, frames: Option<i32>) -> PyResult<Vec<Vec<f64>>> {
        let expected_in = self.num_inputs as usize;
        if inputs.len() != expected_in {
            return Err(PyValueError::new_err(format!(
                "DSP expects {expected_in} input channel(s), got {}",
                inputs.len()
            )));
        }

        // Determine block length from the inputs, or from `frames` when there
        // are no input channels.
        let count: i32 = if expected_in > 0 {
            let len = inputs[0].len();
            if let Some(bad) = inputs.iter().position(|c| c.len() != len) {
                return Err(PyValueError::new_err(format!(
                    "input channel {bad} length {} differs from channel 0 length {len}",
                    inputs[bad].len()
                )));
            }
            i32::try_from(len)
                .map_err(|_| PyValueError::new_err("block length exceeds i32::MAX"))?
        } else {
            frames.ok_or_else(|| {
                PyValueError::new_err("`frames` is required for a DSP with zero inputs")
            })?
        };
        if count < 0 {
            return Err(PyValueError::new_err("`frames` must be non-negative"));
        }

        let num_out = self.num_outputs as usize;

        // Advance the persistent instance; state carries across calls.
        match &mut self.engine {
            Engine::F32(i) => render(i, &inputs, count, num_out),
            Engine::F64(i) => render(i, &inputs, count, num_out),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Dsp(name={:?}, inputs={}, outputs={}, sample_rate={}, precision={:?}, cycle={})",
            self.name,
            self.num_inputs,
            self.num_outputs,
            self.sample_rate,
            self.precision(),
            self.cycle(),
        )
    }
}

/// Compile a Faust `.dsp` source string into a runnable [`Dsp`] handle.
///
/// Set `double=True` for double-precision (`f64`) DSP; the default is single
/// precision (`f32`). Uses the transform fast lane, matching the interpreter
/// FFI's default compilation path.
#[pyfunction]
#[pyo3(signature = (source, name = "FaustDSP", sample_rate = 48000, double = false))]
fn compile(source: &str, name: &str, sample_rate: i32, double: bool) -> PyResult<Dsp> {
    if sample_rate <= 0 {
        return Err(PyValueError::new_err("sample_rate must be positive"));
    }

    let options = InterpOptions {
        module_name: Some(name.to_owned()),
        ..InterpOptions::default()
    };

    let real_type = if double {
        RealType::Float64
    } else {
        RealType::Float32
    };
    let compiler = Compiler::new().with_real_type(real_type);
    let fbc = compiler
        .compile_source_to_interp_with_lane(
            name,
            source,
            &options,
            SignalFirLane::TransformFastLane,
        )
        .map_err(|e| PyValueError::new_err(format!("compile error: {e}")))?;

    // Load the bytecode at the matching precision and wrap it precision-erased.
    let mut cursor = Cursor::new(fbc.into_bytes());
    let engine = if double {
        let factory = read_fbc::<f64>(&mut cursor)
            .map_err(|e| PyValueError::new_err(format!("bytecode load error: {e}")))?;
        Engine::F64(build_instance(factory, sample_rate))
    } else {
        let factory = read_fbc::<f32>(&mut cursor)
            .map_err(|e| PyValueError::new_err(format!("bytecode load error: {e}")))?;
        Engine::F32(build_instance(factory, sample_rate))
    };

    Ok(Dsp::new(engine, sample_rate))
}

/// Return the underlying faust-rs compiler version string.
#[pyfunction]
fn version() -> &'static str {
    Compiler::version()
}

/// The `faust_rs` extension module.
#[pymodule]
fn faust_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(compile, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<Dsp>()?;
    Ok(())
}
