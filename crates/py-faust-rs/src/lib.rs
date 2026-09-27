//! Proof-of-concept PyO3 bindings for faust-rs, over the `faust` crate.
//!
//! Compiles a Faust `.dsp` source string for the bytecode interpreter or the
//! Cranelift JIT and runs it from Python:
//!
//! ```python
//! import faust_rs
//! dsp = faust_rs.compile("process = _, _ : + : *(0.5);", sample_rate=48000)
//! outs = dsp.compute([[0.1, 0.2], [0.3, 0.4]])   # channels -> channels
//! jit = faust_rs.compile("process = _;", backend="cranelift")
//! ```
//!
//! Scope is deliberately narrow (persistent single-block render) to demonstrate
//! the binding path, not to be a full host API. Both single (`f32`) and double
//! (`f64`) precision are supported via the `double=` flag on `compile`.

use std::path::PathBuf;

use faust::{Backend, CompileOptions, ControlKind, Factory, Precision};
use pyo3::buffer::{Element, PyBuffer};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// A DSP control parameter (button, slider, nentry, or bargraph).
///
/// Bargraphs are *outputs* (metering) — readable via `get_param` but not
/// settable. All other kinds are settable inputs.
// `Param` is only ever returned to Python (never taken as an argument), so skip
// the `FromPyObject` derive that `Clone` would otherwise opt into.
#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct Param {
    /// Full UI path, e.g. `/Oscillator/freq`.
    path: String,
    /// Label as declared, without its `[key:value]` metadata, e.g. `freq`.
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

impl From<&faust::Control> for Param {
    fn from(c: &faust::Control) -> Self {
        let kind = match c.kind {
            ControlKind::Button => "button",
            ControlKind::CheckButton => "checkbox",
            ControlKind::HorizontalSlider => "hslider",
            ControlKind::VerticalSlider => "vslider",
            ControlKind::NumEntry => "nentry",
            ControlKind::HorizontalBargraph => "hbargraph",
            ControlKind::VerticalBargraph => "vbargraph",
        };
        Self {
            path: c.path.clone(),
            label: c.label.clone(),
            kind,
            is_input: c.kind.is_writable(),
            init: c.init,
            min: c.min,
            max: c.max,
            step: c.step,
        }
    }
}

fn py_err(e: faust::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// A buffer element width, dispatched to the matching `faust::Dsp` compute.
trait Sample: Element + Copy + Default {
    fn compute(
        dsp: &mut faust::Dsp,
        inputs: &[&[Self]],
        outputs: &mut [&mut [Self]],
    ) -> Result<(), faust::Error>;
}

impl Sample for f32 {
    fn compute(
        dsp: &mut faust::Dsp,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> Result<(), faust::Error> {
        dsp.compute_f32(inputs, outputs)
    }
}

impl Sample for f64 {
    fn compute(
        dsp: &mut faust::Dsp,
        inputs: &[&[f64]],
        outputs: &mut [&mut [f64]],
    ) -> Result<(), faust::Error> {
        dsp.compute_f64(inputs, outputs)
    }
}

/// A compiled Faust DSP program with a persistent, stateful instance.
///
/// A single instance is held across calls, so DSP state — recursive filters,
/// oscillator phase, delay lines — carries from one `compute()` to the next.
/// `init()` runs once at construction; call [`Dsp::reset`] to clear state.
///
/// Audio crosses the Python boundary as `f64` (Python's native float) in
/// `compute`, and at the DSP's precision in `compute_into`.
#[pyclass]
struct Dsp {
    dsp: faust::Dsp,
    params: Vec<Param>,
    /// Blocks rendered since construction; not zeroed by `reset()`.
    cycle: usize,
}

/// Validates a 2-D `(channels, frames)` buffer-protocol object of element `T`,
/// returning the acquired buffer plus its channel and frame counts.
///
/// `PyBuffer::<T>::get` also enforces that the object's element format matches
/// `T` (`float32` for `f32`, `float64` for `f64`), so the copy path below never
/// casts precision. C-contiguity is required so a single bulk copy is valid.
fn view_2d<T: Element>(
    obj: &Bound<'_, PyAny>,
    role: &str,
) -> PyResult<(PyBuffer<T>, usize, usize)> {
    let buf = PyBuffer::<T>::get(obj).map_err(|e| {
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

/// Renders one block in place through buffer-protocol arrays of element `T`.
/// Input samples are bulk-copied out of `inputs`, the block is rendered, and
/// results are bulk-copied into `outputs`. Returns whether a block ran.
fn compute_into_impl<T: Sample>(
    py: Python<'_>,
    dsp: &mut faust::Dsp,
    inputs: &Bound<'_, PyAny>,
    outputs: &Bound<'_, PyAny>,
) -> PyResult<bool> {
    let (num_in, num_out) = (dsp.num_inputs(), dsp.num_outputs());
    let (in_buf, in_ch, in_frames) = view_2d::<T>(inputs, "inputs")?;
    let (out_buf, out_ch, out_frames) = view_2d::<T>(outputs, "outputs")?;

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
        (false, false) => return Ok(false),
    };
    if i32::try_from(frames).is_err() {
        return Err(PyValueError::new_err("block length exceeds i32::MAX"));
    }

    let mut flat_in = vec![T::default(); num_in * frames];
    if !flat_in.is_empty() {
        in_buf.copy_to_slice(py, &mut flat_in)?;
    }
    let in_refs: Vec<&[T]> = if frames == 0 {
        vec![&[]; num_in]
    } else {
        flat_in.chunks(frames).collect()
    };

    let mut flat_out = vec![T::default(); num_out * frames];
    {
        let mut out_refs: Vec<&mut [T]> = if frames == 0 {
            (0..num_out).map(|_| &mut [] as &mut [T]).collect()
        } else {
            flat_out.chunks_mut(frames).collect()
        };
        T::compute(dsp, &in_refs, &mut out_refs).map_err(py_err)?;
    }
    if !flat_out.is_empty() {
        out_buf.copy_from_slice(py, &flat_out)?;
    }
    Ok(true)
}

impl Dsp {
    fn new(dsp: faust::Dsp) -> Self {
        let params = dsp.controls().map(Param::from).collect();
        Self {
            dsp,
            params,
            cycle: 0,
        }
    }

    /// Resolves a parameter key (full path or unambiguous label) to its
    /// `Param`. Errors on unknown or ambiguous keys.
    fn resolve(&self, key: &str) -> PyResult<&Param> {
        if let Some(p) = self.params.iter().find(|p| p.path == key) {
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
}

#[pymethods]
impl Dsp {
    /// Number of audio input channels the DSP expects.
    #[getter]
    fn num_inputs(&self) -> usize {
        self.dsp.num_inputs()
    }

    /// Number of audio output channels the DSP produces.
    #[getter]
    fn num_outputs(&self) -> usize {
        self.dsp.num_outputs()
    }

    /// Render sample rate the instance is initialized with.
    #[getter]
    fn sample_rate(&self) -> i32 {
        self.dsp.sample_rate()
    }

    /// Compiled DSP name.
    #[getter]
    fn name(&self) -> String {
        self.dsp.factory().name().to_owned()
    }

    /// Sample precision the DSP computes with: `"double"` (`f64`) or `"float"`
    /// (`f32`).
    #[getter]
    fn precision(&self) -> &'static str {
        match self.dsp.precision() {
            Precision::F32 => "float",
            Precision::F64 => "double",
        }
    }

    /// Backend running the DSP: `"interp"` or `"cranelift"`.
    #[getter]
    fn backend(&self) -> String {
        self.dsp.backend().to_string()
    }

    /// Total blocks rendered by the persistent instance since construction.
    ///
    /// Monotonic: advances with every `compute()`. It is *not* zeroed by
    /// `reset()` (which clears audio DSP state, not this bookkeeping counter),
    /// so a rising `cycle` evidences that one instance is reused across calls.
    #[getter]
    fn cycle(&self) -> usize {
        self.cycle
    }

    /// Re-initialize the instance, clearing all DSP state (filter memory,
    /// oscillator phase, delay lines) as if freshly compiled. This also resets
    /// every control parameter to its default (`init`) value.
    fn reset(&mut self) {
        let dsp = &mut self.dsp;
        dsp.init(dsp.sample_rate());
    }

    /// The DSP's UI control parameters (sliders, buttons, nentries, bargraphs),
    /// in user-interface order (Faust sorts a group's widgets by label). Each carries its path, kind, and range metadata.
    fn params(&self) -> Vec<Param> {
        self.params.clone()
    }

    /// Read the current value of a control parameter.
    ///
    /// `key` may be the full UI path (e.g. `/Oscillator/freq`) or an
    /// unambiguous label (e.g. `freq`). Works for both input controls and
    /// output bargraphs (the latter reflect the most recent `compute`).
    fn get_param(&self, key: &str) -> PyResult<f64> {
        let path = &self.resolve(key)?.path;
        self.dsp.get(path).map_err(py_err)
    }

    /// Set the value of an input control parameter; takes effect on the next
    /// `compute()`. Bargraphs (outputs) cannot be set.
    ///
    /// `key` may be the full UI path or an unambiguous label. The value is
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
        let path = param.path.clone();
        self.dsp.set(&path, value).map_err(py_err)
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
        let dsp = &mut self.dsp;
        let expected_in = dsp.num_inputs();
        if inputs.len() != expected_in {
            return Err(PyValueError::new_err(format!(
                "DSP expects {expected_in} input channel(s), got {}",
                inputs.len()
            )));
        }

        // Block length comes from the inputs, or from `frames` when there are
        // no input channels.
        let count = if expected_in > 0 {
            let len = inputs[0].len();
            if let Some(bad) = inputs.iter().position(|c| c.len() != len) {
                return Err(PyValueError::new_err(format!(
                    "input channel {bad} length {} differs from channel 0 length {len}",
                    inputs[bad].len()
                )));
            }
            if i32::try_from(len).is_err() {
                return Err(PyValueError::new_err("block length exceeds i32::MAX"));
            }
            len
        } else {
            let frames = frames.ok_or_else(|| {
                PyValueError::new_err("`frames` is required for a DSP with zero inputs")
            })?;
            usize::try_from(frames)
                .map_err(|_| PyValueError::new_err("`frames` must be non-negative"))?
        };

        let in_refs: Vec<&[f64]> = inputs.iter().map(Vec::as_slice).collect();
        let mut outs = vec![vec![0.0; count]; dsp.num_outputs()];
        let mut out_refs: Vec<&mut [f64]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        dsp.compute_f64(&in_refs, &mut out_refs).map_err(py_err)?;
        self.cycle += 1;
        Ok(outs)
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
        let dsp = &mut self.dsp;
        let ran = match dsp.precision() {
            Precision::F32 => compute_into_impl::<f32>(py, dsp, inputs, outputs)?,
            Precision::F64 => compute_into_impl::<f64>(py, dsp, inputs, outputs)?,
        };
        if ran {
            self.cycle += 1;
        }
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "Dsp(name={:?}, backend={:?}, inputs={}, outputs={}, sample_rate={}, precision={:?}, cycle={})",
            self.name(),
            self.backend(),
            self.num_inputs(),
            self.num_outputs(),
            self.sample_rate(),
            self.precision(),
            self.cycle,
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

/// Compile a Faust `.dsp` source string into a runnable [`Dsp`] handle.
///
/// `backend` is `"interp"` (the bytecode interpreter, the default) or
/// `"cranelift"` (native code through the Cranelift JIT). Cranelift does not
/// yet lower every program; one it cannot run raises `ValueError`.
///
/// Set `double=True` for double-precision (`f64`) DSP; the default is single
/// precision (`f32`).
///
/// `search_paths` is an optional list of directories in which to resolve
/// `import("...")` directives (e.g. a directory containing the Faust standard
/// libraries so `import("stdfaust.lib")` works). Directories listed in the
/// `FAUST_LIB_PATH` environment variable are appended automatically.
#[pyfunction]
#[pyo3(signature = (source, name = "FaustDSP", sample_rate = 48000, double = false, search_paths = None, backend = "interp"))]
fn compile(
    py: Python<'_>,
    source: &str,
    name: &str,
    sample_rate: i32,
    double: bool,
    search_paths: Option<Vec<String>>,
    backend: &str,
) -> PyResult<Dsp> {
    if sample_rate <= 0 {
        return Err(PyValueError::new_err("sample_rate must be positive"));
    }
    let backend = match backend {
        "interp" => Backend::Interp,
        "cranelift" => Backend::Cranelift,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown backend {other:?}; expected \"interp\" or \"cranelift\""
            )));
        }
    };

    // Effective import search paths: explicit argument first, then any
    // directories from FAUST_LIB_PATH (Faust's conventional env var).
    let mut import_dirs: Vec<PathBuf> = search_paths
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if let Some(env_paths) = std::env::var_os("FAUST_LIB_PATH") {
        import_dirs.extend(std::env::split_paths(&env_paths));
    }
    let options = CompileOptions {
        backend,
        precision: if double {
            Precision::F64
        } else {
            Precision::F32
        },
        import_dirs,
        ..CompileOptions::default()
    };

    let source = source.to_owned();
    let name = name.to_owned();

    // Run the deeply-recursive compile on a worker thread with the workspace
    // stack contract (see `COMPILE_STACK_SIZE`), releasing the GIL while it runs
    // since the pipeline touches no Python state.
    let dsp = py.detach(move || {
        std::thread::Builder::new()
            .name("faust-rs-compile".to_owned())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn(move || Factory::from_source(&name, &source, &options)?.instantiate(sample_rate))
            .map_err(|e| PyValueError::new_err(format!("failed to spawn compile thread: {e}")))?
            .join()
            .map_err(|_| PyValueError::new_err("compile thread panicked"))?
            .map_err(py_err)
    })?;

    Ok(Dsp::new(dsp))
}

/// Return the underlying faust-rs version string.
#[pyfunction]
fn version() -> &'static str {
    faust::version()
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
