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
//! Scope is deliberately narrow (single precision `f32`, one-shot render) to
//! demonstrate the binding path, not to be a full host API.

use std::io::Cursor;

use codegen::backends::interp::{FbcDspFactory, FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, RealType, SignalFirLane};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// A compiled Faust DSP program plus its render sample rate.
///
/// Holds an owned `f32` FBC factory. Each `compute` call instantiates a fresh
/// interpreter instance over that factory, so the object is a reusable,
/// stateless-between-calls program handle.
#[pyclass]
struct Dsp {
    factory: FbcDspFactory<f32>,
    sample_rate: i32,
}

#[pymethods]
impl Dsp {
    /// Number of audio input channels the DSP expects.
    #[getter]
    fn num_inputs(&self) -> i32 {
        self.factory.num_inputs
    }

    /// Number of audio output channels the DSP produces.
    #[getter]
    fn num_outputs(&self) -> i32 {
        self.factory.num_outputs
    }

    /// Render sample rate the instance is initialized with.
    #[getter]
    fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    /// Compiled DSP name.
    #[getter]
    fn name(&self) -> String {
        self.factory.name.clone()
    }

    /// Render one block of audio.
    ///
    /// `inputs` is a list of input channels (each a list of `f32` samples); all
    /// channels must share the same length. For a DSP with zero inputs, pass an
    /// empty list and set `frames`. Returns a list of output channels.
    #[pyo3(signature = (inputs, frames = None))]
    fn compute(&mut self, inputs: Vec<Vec<f32>>, frames: Option<i32>) -> PyResult<Vec<Vec<f32>>> {
        let expected_in = self.factory.num_inputs as usize;
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

        let num_out = self.factory.num_outputs as usize;
        let mut out_bufs: Vec<Vec<f32>> = vec![vec![0.0f32; count as usize]; num_out];

        // The interpreter borrows the factory mutably for the render lifetime.
        let mut inst = FbcDspInstance::new(&mut self.factory);
        inst.init(self.sample_rate);

        let in_refs: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
        let mut out_refs: Vec<&mut [f32]> = out_bufs.iter_mut().map(Vec::as_mut_slice).collect();

        inst.try_compute(count, &in_refs, &mut out_refs)
            .map_err(|e| PyValueError::new_err(format!("interpreter runtime error: {e}")))?;

        Ok(out_bufs)
    }

    fn __repr__(&self) -> String {
        format!(
            "Dsp(name={:?}, inputs={}, outputs={}, sample_rate={})",
            self.factory.name, self.factory.num_inputs, self.factory.num_outputs, self.sample_rate
        )
    }
}

/// Compile a Faust `.dsp` source string into a runnable [`Dsp`] handle.
///
/// Single precision (`f32`) only in this proof of concept. Uses the transform
/// fast lane, matching the interpreter FFI's default compilation path.
#[pyfunction]
#[pyo3(signature = (source, name = "FaustDSP", sample_rate = 48000))]
fn compile(source: &str, name: &str, sample_rate: i32) -> PyResult<Dsp> {
    if sample_rate <= 0 {
        return Err(PyValueError::new_err("sample_rate must be positive"));
    }

    let options = InterpOptions {
        module_name: Some(name.to_owned()),
        ..InterpOptions::default()
    };

    let compiler = Compiler::new().with_real_type(RealType::Float32);
    let fbc = compiler
        .compile_source_to_interp_with_lane(name, source, &options, SignalFirLane::TransformFastLane)
        .map_err(|e| PyValueError::new_err(format!("compile error: {e}")))?;

    let mut cursor = Cursor::new(fbc.into_bytes());
    let factory = read_fbc::<f32>(&mut cursor)
        .map_err(|e| PyValueError::new_err(format!("bytecode load error: {e}")))?;

    Ok(Dsp {
        factory,
        sample_rate,
    })
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
