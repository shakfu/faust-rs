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
use std::path::PathBuf;

use codegen::backends::interp::{
    FbcDspFactory, FbcOpcode, FbcReal, FbcUiInstruction, InterpOptions, OwnedFbcDspInstance,
    read_fbc,
};
use compiler::{Compiler, RealType, SignalFirLane};
use pyo3::buffer::{Element, PyBuffer};
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

/// A DSP control parameter (button, slider, nentry, or bargraph).
///
/// Bargraphs are *outputs* (metering) — readable via `get_param` but not
/// settable. All other kinds are settable inputs. `offset` is the real-heap
/// zone the interpreter binds the control to.
// `Param` is only ever returned to Python (never taken as an argument), so skip
// the `FromPyObject` derive that `Clone` would otherwise opt into.
#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct Param {
    /// Full UI path, e.g. `/Oscillator/freq`.
    path: String,
    /// Leaf label, e.g. `freq`.
    label: String,
    /// Widget kind: `button`, `checkbox`, `hslider`, `vslider`, `nentry`,
    /// `hbargraph`, or `vbargraph`.
    kind: &'static str,
    /// Whether the control is a settable input (false for bargraphs).
    is_input: bool,
    init: f64,
    min: f64,
    max: f64,
    step: f64,
    /// Real-heap zone offset.
    offset: i32,
}

#[pymethods]
impl Param {
    fn __repr__(&self) -> String {
        format!(
            "Param(path={:?}, kind={:?}, init={}, min={}, max={}, step={}, input={})",
            self.path, self.kind, self.init, self.min, self.max, self.step, self.is_input
        )
    }
}

/// Widget kind name for a UI opcode, or `None` if it is not a control.
fn control_kind(opcode: FbcOpcode) -> Option<&'static str> {
    Some(match opcode {
        FbcOpcode::AddButton => "button",
        FbcOpcode::AddCheckButton => "checkbox",
        FbcOpcode::AddHorizontalSlider => "hslider",
        FbcOpcode::AddVerticalSlider => "vslider",
        FbcOpcode::AddNumEntry => "nentry",
        FbcOpcode::AddHorizontalBargraph => "hbargraph",
        FbcOpcode::AddVerticalBargraph => "vbargraph",
        _ => return None,
    })
}

/// Walks the interpreter UI instruction list into a flat parameter list,
/// building each control's full path from the enclosing box labels.
fn collect_params<R: FbcReal>(ui: &[FbcUiInstruction<R>]) -> Vec<Param> {
    let mut params = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    for instr in ui {
        match instr.opcode {
            FbcOpcode::OpenVerticalBox | FbcOpcode::OpenHorizontalBox | FbcOpcode::OpenTabBox => {
                stack.push(&instr.label)
            }
            FbcOpcode::CloseBox => {
                stack.pop();
            }
            opcode => {
                let Some(kind) = control_kind(opcode) else {
                    continue; // Declare, AddSoundfile, etc.
                };
                let mut path = String::new();
                for segment in &stack {
                    path.push('/');
                    path.push_str(segment);
                }
                path.push('/');
                path.push_str(&instr.label);
                let is_bargraph = matches!(
                    opcode,
                    FbcOpcode::AddHorizontalBargraph | FbcOpcode::AddVerticalBargraph
                );
                params.push(Param {
                    path,
                    label: instr.label.clone(),
                    kind,
                    is_input: !is_bargraph,
                    init: instr.init.to_f64(),
                    min: instr.min.to_f64(),
                    max: instr.max.to_f64(),
                    step: instr.step.to_f64(),
                    offset: instr.offset,
                });
            }
        }
    }
    params
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
    params: Vec<Param>,
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

/// Validates a 2-D `(channels, frames)` buffer-protocol object of precision `R`,
/// returning the acquired buffer plus its channel and frame counts.
///
/// `PyBuffer::<R>::get` also enforces that the object's element format matches
/// `R` (`float32` for `f32`, `float64` for `f64`), so the copy path below never
/// casts precision. C-contiguity is required so a single bulk copy is valid.
fn view_2d<R: Element>(
    obj: &Bound<'_, PyAny>,
    role: &str,
) -> PyResult<(PyBuffer<R>, usize, usize)> {
    let buf = PyBuffer::<R>::get(obj).map_err(|e| {
        PyValueError::new_err(format!(
            "{role} must be a contiguous buffer whose dtype matches the DSP precision: {e}"
        ))
    })?;
    if buf.dimensions() != 2 {
        return Err(PyValueError::new_err(format!(
            "{role} must be a 2-D (channels, frames) buffer, got {}-D",
            buf.dimensions()
        )));
    }
    if !buf.is_c_contiguous() {
        return Err(PyValueError::new_err(format!(
            "{role} must be C-contiguous"
        )));
    }
    let (channels, frames) = (buf.shape()[0], buf.shape()[1]);
    Ok((buf, channels, frames))
}

/// Renders one block in place through buffer-protocol arrays for an instance of
/// precision `R`. Input samples are bulk-copied out of `inputs`, the block is
/// rendered, and results are bulk-copied into `outputs` — no per-sample Python
/// object marshaling and no precision cast (the dtype is required to match `R`).
fn compute_into_impl<R: FbcReal + Element>(
    py: Python<'_>,
    instance: &mut OwnedFbcDspInstance<R>,
    inputs: &Bound<'_, PyAny>,
    outputs: &Bound<'_, PyAny>,
    num_in: usize,
    num_out: usize,
) -> PyResult<()> {
    let (in_buf, in_ch, in_frames) = view_2d::<R>(inputs, "inputs")?;
    let (out_buf, out_ch, out_frames) = view_2d::<R>(outputs, "outputs")?;

    if in_ch != num_in {
        return Err(PyValueError::new_err(format!(
            "inputs has {in_ch} channel(s), DSP expects {num_in}"
        )));
    }
    if out_ch != num_out {
        return Err(PyValueError::new_err(format!(
            "outputs has {out_ch} channel(s), DSP produces {num_out}"
        )));
    }
    if out_buf.readonly() {
        return Err(PyValueError::new_err("outputs buffer is read-only"));
    }

    // Frame count is authoritative from whichever side carries channels; if both
    // do, they must agree. A DSP with neither inputs nor outputs is a no-op.
    let frames = match (num_in > 0, num_out > 0) {
        (true, true) if in_frames != out_frames => {
            return Err(PyValueError::new_err(format!(
                "inputs has {in_frames} frame(s) but outputs has {out_frames}"
            )));
        }
        (true, _) => in_frames,
        (false, true) => out_frames,
        (false, false) => return Ok(()),
    };
    let count = i32::try_from(frames)
        .map_err(|_| PyValueError::new_err("block length exceeds i32::MAX"))?;

    // Copy inputs into contiguous per-channel storage (one memcpy for the whole
    // buffer), then view it as `num_in` channel slices.
    let mut flat_in = vec![R::default(); num_in * frames];
    if !flat_in.is_empty() {
        in_buf.copy_to_slice(py, &mut flat_in)?;
    }
    let in_refs: Vec<&[R]> = if frames == 0 {
        (0..num_in).map(|_| &[] as &[R]).collect()
    } else {
        flat_in.chunks(frames).collect()
    };

    // Render into contiguous output storage, then bulk-copy into the caller's
    // writable buffer.
    let mut flat_out = vec![R::default(); num_out * frames];
    {
        let mut out_refs: Vec<&mut [R]> = if frames == 0 {
            (0..num_out).map(|_| &mut [] as &mut [R]).collect()
        } else {
            flat_out.chunks_mut(frames).collect()
        };
        instance
            .try_compute(count, &in_refs, &mut out_refs)
            .map_err(|e| PyValueError::new_err(format!("interpreter runtime error: {e}")))?;
    }
    if !flat_out.is_empty() {
        out_buf.copy_from_slice(py, &flat_out)?;
    }
    Ok(())
}

impl Dsp {
    /// Wraps a precision-erased engine, caching audio-layout metadata and the
    /// UI parameter list.
    fn new(engine: Engine, sample_rate: i32) -> Self {
        let (num_inputs, num_outputs, name, double, params) = match &engine {
            Engine::F32(i) => (
                i.get_num_inputs(),
                i.get_num_outputs(),
                i.factory().name.clone(),
                false,
                collect_params(i.ui_instructions()),
            ),
            Engine::F64(i) => (
                i.get_num_inputs(),
                i.get_num_outputs(),
                i.factory().name.clone(),
                true,
                collect_params(i.ui_instructions()),
            ),
        };
        Self {
            engine,
            sample_rate,
            num_inputs,
            num_outputs,
            name,
            double,
            params,
        }
    }

    /// Resolves a parameter key (full path or unambiguous leaf label) to its
    /// `Param`. Errors on unknown or ambiguous keys.
    fn resolve(&self, key: &str) -> PyResult<&Param> {
        let mut by_path = self.params.iter().filter(|p| p.path == key);
        if let Some(p) = by_path.next() {
            // Paths are unique in a well-formed UI, so first match wins.
            return Ok(p);
        }
        let mut by_label = self.params.iter().filter(|p| p.label == key);
        match (by_label.next(), by_label.next()) {
            (Some(p), None) => Ok(p),
            (None, _) => {
                let available: Vec<&str> = self.params.iter().map(|p| p.path.as_str()).collect();
                Err(PyValueError::new_err(format!(
                    "unknown parameter {key:?}; available: {available:?}"
                )))
            }
            (Some(_), Some(_)) => Err(PyValueError::new_err(format!(
                "ambiguous parameter label {key:?}; use the full path"
            ))),
        }
    }

    /// Reads a real-heap zone (precision-erased to `f64`).
    fn get_zone(&self, offset: i32) -> Option<f64> {
        match &self.engine {
            Engine::F32(i) => i.get_real_zone(offset).map(FbcReal::to_f64),
            Engine::F64(i) => i.get_real_zone(offset).map(FbcReal::to_f64),
        }
    }

    /// Writes a real-heap zone, casting to the engine precision.
    fn set_zone(&mut self, offset: i32, value: f64) -> bool {
        match &mut self.engine {
            Engine::F32(i) => i.set_real_zone(offset, f32::from_f64(value)),
            Engine::F64(i) => i.set_real_zone(offset, f64::from_f64(value)),
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
    /// oscillator phase, delay lines) as if freshly compiled. This also resets
    /// every control parameter to its default (`init`) value.
    fn reset(&mut self) {
        match &mut self.engine {
            Engine::F32(i) => i.init(self.sample_rate),
            Engine::F64(i) => i.init(self.sample_rate),
        }
    }

    /// The DSP's UI control parameters (sliders, buttons, nentries, bargraphs),
    /// in UI-declaration order. Each carries its path, kind, and range metadata.
    fn params(&self) -> Vec<Param> {
        self.params.clone()
    }

    /// Read the current value of a control parameter.
    ///
    /// `key` may be the full UI path (e.g. `/Oscillator/freq`) or an
    /// unambiguous leaf label (e.g. `freq`). Works for both input controls and
    /// output bargraphs (the latter reflect the most recent `compute`).
    fn get_param(&self, key: &str) -> PyResult<f64> {
        let offset = self.resolve(key)?.offset;
        self.get_zone(offset)
            .ok_or_else(|| PyValueError::new_err(format!("parameter {key:?} zone out of range")))
    }

    /// Set the value of an input control parameter; takes effect on the next
    /// `compute()`. Bargraphs (outputs) cannot be set.
    ///
    /// `key` may be the full UI path or an unambiguous leaf label. The value is
    /// not clamped to the control's declared `[min, max]` range (matching
    /// Faust's `setParamValue` semantics).
    fn set_param(&mut self, key: &str, value: f64) -> PyResult<()> {
        let param = self.resolve(key)?;
        if !param.is_input {
            return Err(PyValueError::new_err(format!(
                "parameter {:?} is an output ({}) and cannot be set",
                param.path, param.kind
            )));
        }
        let offset = param.offset;
        if self.set_zone(offset, value) {
            Ok(())
        } else {
            Err(PyValueError::new_err(format!(
                "parameter {key:?} zone out of range"
            )))
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

    /// Render one block **in place** through buffer-protocol arrays, advancing
    /// the persistent instance state.
    ///
    /// This is the zero-marshaling counterpart to [`Dsp::compute`]: instead of
    /// Python lists (which box every sample as a `PyFloat`), it reads and writes
    /// contiguous native buffers, so large blocks avoid per-sample conversion.
    ///
    /// Both `inputs` and `outputs` are 2-D `(channels, frames)` C-contiguous
    /// buffer-protocol objects — a NumPy array, a shaped `memoryview`, or any
    /// object exposing the buffer protocol. Their **dtype must match the DSP
    /// precision**: `float32` for a `"float"` DSP, `float64` for a `"double"`
    /// one (mismatches raise, they are never silently cast). `inputs` must have
    /// `num_inputs` rows, `outputs` must have `num_outputs` rows and be
    /// writable, and (when both carry channels) their frame counts must agree.
    /// The rendered block is written into `outputs` in place; nothing is
    /// returned.
    ///
    /// For a zero-input DSP, pass a `(0, frames)` input array — the frame count
    /// is then taken from `outputs`.
    #[pyo3(signature = (inputs, outputs))]
    fn compute_into(
        &mut self,
        py: Python<'_>,
        inputs: &Bound<'_, PyAny>,
        outputs: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let num_in = self.num_inputs as usize;
        let num_out = self.num_outputs as usize;
        match &mut self.engine {
            Engine::F32(i) => compute_into_impl::<f32>(py, i, inputs, outputs, num_in, num_out),
            Engine::F64(i) => compute_into_impl::<f64>(py, i, inputs, outputs, num_in, num_out),
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

/// 64 MiB worker-thread stack for the compile path.
///
/// The evaluator's structural-lowering pass recurses deeply for large programs
/// (notably anything that expands `import("stdfaust.lib")`), and its guarded
/// recursion budgets are sized against the workspace's 64 MiB stack contract
/// (see `compiler::main`). Python calls this extension on its main thread, whose
/// stack (~8 MiB on CPython) is far below that contract, so a stdfaust-based
/// compile overflows it. Running the compile on a thread with the contract's
/// headroom keeps the binding within the same envelope as every other embedder.
const COMPILE_STACK_SIZE: usize = 64 * 1024 * 1024;

/// The pure-Rust compile pipeline: source string -> FBC bytecode -> precision-
/// erased owning instance -> `Dsp`. Runs entirely off the GIL on the worker
/// thread; errors come back as strings for the caller to wrap in `ValueError`.
fn compile_pipeline(
    source: String,
    name: String,
    sample_rate: i32,
    double: bool,
    paths: Vec<PathBuf>,
) -> Result<Dsp, String> {
    let options = InterpOptions {
        module_name: Some(name.clone()),
        ..InterpOptions::default()
    };

    let real_type = if double {
        RealType::Float64
    } else {
        RealType::Float32
    };
    let compiler = Compiler::new().with_real_type(real_type);
    let fbc = compiler
        .compile_source_to_interp_with_lane_and_search_paths(
            &name,
            &source,
            &paths,
            &options,
            SignalFirLane::TransformFastLane,
        )
        .map_err(|e| format!("compile error: {e}"))?;

    // Load the bytecode at the matching precision and wrap it precision-erased.
    let mut cursor = Cursor::new(fbc.into_bytes());
    let engine = if double {
        let factory =
            read_fbc::<f64>(&mut cursor).map_err(|e| format!("bytecode load error: {e}"))?;
        Engine::F64(build_instance(factory, sample_rate))
    } else {
        let factory =
            read_fbc::<f32>(&mut cursor).map_err(|e| format!("bytecode load error: {e}"))?;
        Engine::F32(build_instance(factory, sample_rate))
    };

    Ok(Dsp::new(engine, sample_rate))
}

/// Compile a Faust `.dsp` source string into a runnable [`Dsp`] handle.
///
/// Set `double=True` for double-precision (`f64`) DSP; the default is single
/// precision (`f32`). Uses the transform fast lane, matching the interpreter
/// FFI's default compilation path.
///
/// `search_paths` is an optional list of directories in which to resolve
/// `import("...")` directives (e.g. a directory containing the Faust standard
/// libraries so `import("stdfaust.lib")` works). Directories listed in the
/// `FAUST_LIB_PATH` environment variable are appended automatically.
#[pyfunction]
#[pyo3(signature = (source, name = "FaustDSP", sample_rate = 48000, double = false, search_paths = None))]
fn compile(
    py: Python<'_>,
    source: &str,
    name: &str,
    sample_rate: i32,
    double: bool,
    search_paths: Option<Vec<String>>,
) -> PyResult<Dsp> {
    if sample_rate <= 0 {
        return Err(PyValueError::new_err("sample_rate must be positive"));
    }

    // Effective import search paths: explicit argument first, then any
    // directories from FAUST_LIB_PATH (Faust's conventional env var).
    let mut paths: Vec<PathBuf> = search_paths
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if let Some(env_paths) = std::env::var_os("FAUST_LIB_PATH") {
        paths.extend(std::env::split_paths(&env_paths));
    }

    let source = source.to_owned();
    let name = name.to_owned();

    // Run the deeply-recursive compile on a worker thread with the workspace
    // stack contract (see `COMPILE_STACK_SIZE`), releasing the GIL while it runs
    // since the pipeline touches no Python state.
    let result = py.detach(move || {
        std::thread::Builder::new()
            .name("faust-rs-compile".to_owned())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn(move || compile_pipeline(source, name, sample_rate, double, paths))
            .map_err(|e| format!("failed to spawn compile thread: {e}"))?
            .join()
            .map_err(|_| "compile thread panicked".to_owned())?
    });

    result.map_err(PyValueError::new_err)
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
    m.add_class::<Param>()?;
    Ok(())
}
