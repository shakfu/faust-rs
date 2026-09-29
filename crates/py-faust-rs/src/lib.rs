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
//! factory = faust_rs.Factory("process = _;")  # compile once
//! a, b = factory.create_dsp_instance(), factory.create_dsp_instance()
//! ```
//!
//! Parameter lookup and errors follow the `faust` facade, which follows the C++
//! `dsp`/`MapUI` contract: the binding adds no name resolution of its own.

use std::path::PathBuf;

use faust::{Backend, CompileOptions, ErrorKind, ParamKind, Precision};
use pyo3::buffer::{Element, PyBuffer};
use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

create_exception!(
    faust_rs,
    FaustError,
    PyValueError,
    "Base class of every error the `faust` facade reports."
);
create_exception!(
    faust_rs,
    CompileError,
    FaustError,
    "The program did not compile."
);
create_exception!(
    faust_rs,
    InstantiateError,
    FaustError,
    "The backend could not instantiate the program."
);
create_exception!(
    faust_rs,
    UnknownParamError,
    FaustError,
    "No parameter has this path, shortname or label."
);
create_exception!(
    faust_rs,
    ReadOnlyParamError,
    FaustError,
    "The parameter is a bargraph, written by the DSP only."
);
create_exception!(
    faust_rs,
    BuffersError,
    FaustError,
    "The audio buffers do not match the DSP's channel or frame counts."
);

/// A DSP control parameter (button, slider, nentry, or bargraph).
///
/// Bargraphs are *outputs* (metering) — readable via `get_param` but not
/// settable. All other kinds are settable inputs.
#[pyclass(frozen, get_all)]
struct Param {
    /// Full UI path, e.g. `/Oscillator/freq`.
    path: String,
    /// Shortest unambiguous name, as the C++ `MapUI` builds it, e.g.
    /// `freq`, or `osc0_freq` when another parameter is also named `freq`.
    shortname: String,
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
    /// The `[key:value]` metadata declared on the widget, in order.
    metadata: Vec<(String, String)>,
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

impl From<&faust::Param> for Param {
    fn from(c: &faust::Param) -> Self {
        let kind = match c.kind {
            ParamKind::Button => "button",
            ParamKind::CheckButton => "checkbox",
            ParamKind::HorizontalSlider => "hslider",
            ParamKind::VerticalSlider => "vslider",
            ParamKind::NumEntry => "nentry",
            ParamKind::HorizontalBargraph => "hbargraph",
            ParamKind::VerticalBargraph => "vbargraph",
        };
        Self {
            path: c.path.clone(),
            shortname: c.shortname.clone(),
            label: c.label.clone(),
            kind,
            is_input: c.kind.is_writable(),
            init: c.init,
            min: c.min,
            max: c.max,
            step: c.step,
            metadata: c.metadata.clone(),
        }
    }
}

fn precision_name(precision: Precision) -> &'static str {
    match precision {
        Precision::F32 => "float",
        Precision::F64 => "double",
    }
}

/// Raises the exception class of the facade's `ErrorKind`.
fn py_err(e: faust::Error) -> PyErr {
    let msg = e.to_string();
    match e.kind {
        ErrorKind::Compile => CompileError::new_err(msg),
        ErrorKind::Instantiate => InstantiateError::new_err(msg),
        ErrorKind::UnknownParam => UnknownParamError::new_err(msg),
        ErrorKind::ReadOnlyParam => ReadOnlyParamError::new_err(msg),
        ErrorKind::Buffers => BuffersError::new_err(msg),
        _ => FaustError::new_err(msg),
    }
}

/// A buffer element width the facade computes with and numpy can view.
trait Sample: faust::Sample + Element + Copy + Default {}

impl<T: faust::Sample + Element + Copy + Default> Sample for T {}

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
        BuffersError::new_err(format!(
            "{role} must be a contiguous buffer whose dtype matches the DSP precision: {e}"
        ))
    })?;
    if buf.dimensions() != 2 {
        return Err(BuffersError::new_err(format!(
            "{role} must be a 2-D (channels, frames) buffer, got {}-D",
            buf.dimensions()
        )));
    }
    if !buf.is_c_contiguous() {
        return Err(BuffersError::new_err(format!(
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
    let (num_in, num_out) = (dsp.get_num_inputs(), dsp.get_num_outputs());
    let (in_buf, in_ch, in_frames) = view_2d::<T>(inputs, "inputs")?;
    let (out_buf, out_ch, out_frames) = view_2d::<T>(outputs, "outputs")?;

    if in_ch != num_in {
        return Err(BuffersError::new_err(format!(
            "inputs has {in_ch} channel(s), DSP expects {num_in}"
        )));
    }
    if out_ch != num_out {
        return Err(BuffersError::new_err(format!(
            "outputs has {out_ch} channel(s), DSP produces {num_out}"
        )));
    }
    if out_buf.readonly() {
        return Err(BuffersError::new_err("outputs buffer is read-only"));
    }

    // Frame count is authoritative from whichever side carries channels; if both
    // do, they must agree. A DSP with neither inputs nor outputs is a no-op.
    let frames = match (num_in > 0, num_out > 0) {
        (true, true) if in_frames != out_frames => {
            return Err(BuffersError::new_err(format!(
                "inputs has {in_frames} frame(s) but outputs has {out_frames}"
            )));
        }
        (true, _) => in_frames,
        (false, true) => out_frames,
        (false, false) => return Ok(false),
    };
    if i32::try_from(frames).is_err() {
        return Err(BuffersError::new_err("block length exceeds i32::MAX"));
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
        dsp.compute(frames, &in_refs, &mut out_refs)
            .map_err(py_err)?;
    }
    if !flat_out.is_empty() {
        out_buf.copy_from_slice(py, &flat_out)?;
    }
    Ok(true)
}

impl Dsp {
    fn new(dsp: faust::Dsp) -> Self {
        Self { dsp, cycle: 0 }
    }

    /// `py_err`, with the known paths listed when `key` names no parameter.
    fn param_err(&self, key: &str, e: faust::Error) -> PyErr {
        if e.kind != ErrorKind::UnknownParam {
            return py_err(e);
        }
        let available: Vec<&str> = self.dsp.params().map(|p| p.path.as_str()).collect();
        UnknownParamError::new_err(format!(
            "unknown parameter {key:?}; available: {available:?}"
        ))
    }
}

#[pymethods]
impl Dsp {
    /// Number of audio input channels the DSP expects.
    #[getter]
    fn num_inputs(&self) -> usize {
        self.dsp.get_num_inputs()
    }

    /// Number of audio output channels the DSP produces.
    #[getter]
    fn num_outputs(&self) -> usize {
        self.dsp.get_num_outputs()
    }

    /// Render sample rate the instance is initialized with.
    #[getter]
    fn sample_rate(&self) -> i32 {
        self.dsp.get_sample_rate()
    }

    /// Compiled DSP name.
    #[getter]
    fn name(&self) -> String {
        self.dsp.factory().get_name().to_owned()
    }

    /// Sample precision the DSP computes with: `"double"` (`f64`) or `"float"`
    /// (`f32`).
    #[getter]
    fn precision(&self) -> &'static str {
        precision_name(self.dsp.precision())
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
        dsp.init(dsp.get_sample_rate());
    }

    /// The program this instance runs; `create_dsp_instance` on it makes
    /// siblings without recompiling.
    #[getter]
    fn factory(&self) -> Factory {
        Factory {
            factory: self.dsp.factory(),
        }
    }

    /// The `declare` (key, value) metadata of the program, plus the
    /// backend's own entries, in declaration order. Keys may repeat.
    fn metadata(&self) -> Vec<(String, String)> {
        self.dsp.metadata()
    }

    /// The DSP's UI control parameters (sliders, buttons, nentries, bargraphs),
    /// in user-interface order (Faust sorts a group's widgets by label).
    fn params(&self) -> Vec<Param> {
        self.dsp.params().map(Param::from).collect()
    }

    /// Read the current value of a control parameter. For a bargraph, the
    /// value the most recent `compute` wrote.
    ///
    /// `key` is looked up as the C++ `MapUI` does: as a path
    /// (`/synth/osc0/freq`), then a shortname (`osc0_freq`), then a label
    /// (`freq`). A label several parameters share designates the last one
    /// declared.
    fn get_param(&self, key: &str) -> PyResult<f64> {
        self.dsp
            .get_param_value(key)
            .map_err(|e| self.param_err(key, e))
    }

    /// Set an input control parameter; takes effect on the next `compute()`.
    /// `key` is looked up as by `get_param`. The value is not clamped to
    /// `[min, max]`, as with Faust's `setParamValue`.
    ///
    /// Raises `ReadOnlyParamError` for a bargraph.
    fn set_param(&mut self, key: &str, value: f64) -> PyResult<()> {
        match self.dsp.set_param_value(key, value) {
            Ok(()) => Ok(()),
            Err(e) => Err(self.param_err(key, e)),
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
        let dsp = &mut self.dsp;
        let expected_in = dsp.get_num_inputs();
        if inputs.len() != expected_in {
            return Err(BuffersError::new_err(format!(
                "DSP expects {expected_in} input channel(s), got {}",
                inputs.len()
            )));
        }

        // Block length comes from the inputs, or from `frames` when there are
        // no input channels.
        let count = if expected_in > 0 {
            let len = inputs[0].len();
            if let Some(bad) = inputs.iter().position(|c| c.len() != len) {
                return Err(BuffersError::new_err(format!(
                    "input channel {bad} length {} differs from channel 0 length {len}",
                    inputs[bad].len()
                )));
            }
            if i32::try_from(len).is_err() {
                return Err(BuffersError::new_err("block length exceeds i32::MAX"));
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
        let mut outs = vec![vec![0.0; count]; dsp.get_num_outputs()];
        let mut out_refs: Vec<&mut [f64]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        dsp.compute(count, &in_refs, &mut out_refs)
            .map_err(py_err)?;
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

/// Compiler flags that set the precision. The facade sizes buffers from
/// `CompileOptions::precision`; given `-double` in `args` on an `f32` program,
/// Cranelift writes `f64` samples past the host's output buffer.
const PRECISION_FLAGS: [&str; 4] = ["-single", "-double", "--single", "--double"];

fn check_sample_rate(sample_rate: i32) -> PyResult<()> {
    if sample_rate <= 0 {
        return Err(PyValueError::new_err("sample_rate must be positive"));
    }
    Ok(())
}

/// What a factory is compiled from.
enum Source {
    Text { name: String, source: String },
    File(PathBuf),
}

/// Compiles `source` on a worker thread with the workspace stack contract
/// (see `COMPILE_STACK_SIZE`), the GIL released.
fn build_factory(
    py: Python<'_>,
    source: Source,
    double: bool,
    search_paths: Option<Vec<String>>,
    backend: &str,
    args: Option<Vec<String>>,
    opt_level: i32,
) -> PyResult<faust::Factory> {
    let backend = match backend {
        "interp" => Backend::Interp,
        "cranelift" => Backend::Cranelift,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown backend {other:?}; expected \"interp\" or \"cranelift\""
            )));
        }
    };
    if !(0..=3).contains(&opt_level) {
        return Err(PyValueError::new_err(format!(
            "opt_level must be 0 to 3, got {opt_level}"
        )));
    }
    let args = args.unwrap_or_default();
    if let Some(flag) = args.iter().find(|a| PRECISION_FLAGS.contains(&a.as_str())) {
        return Err(PyValueError::new_err(format!(
            "{flag:?} is not accepted in args; use double= to set the precision"
        )));
    }

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
        opt_level,
        args,
    };

    py.detach(move || {
        std::thread::Builder::new()
            .name("faust-rs-compile".to_owned())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn(move || match source {
                Source::Text { name, source } => {
                    faust::Factory::from_source(&name, &source, &options)
                }
                Source::File(path) => faust::Factory::from_file(&path, &options),
            })
            .map_err(|e| PyValueError::new_err(format!("failed to spawn compile thread: {e}")))?
            .join()
            .map_err(|_| PyValueError::new_err("compile thread panicked"))?
            .map_err(py_err)
    })
}

/// A compiled Faust program. Compile once, then create any number of
/// independent `Dsp` instances with `create_dsp_instance`.
///
/// Takes the arguments of [`compile`] except `sample_rate`, which each
/// instance chooses.
#[pyclass(frozen)]
struct Factory {
    factory: faust::Factory,
}

#[pymethods]
impl Factory {
    #[new]
    #[pyo3(signature = (source, name = "FaustDSP", double = false, search_paths = None, backend = "interp", args = None, opt_level = 0))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        source: &str,
        name: &str,
        double: bool,
        search_paths: Option<Vec<String>>,
        backend: &str,
        args: Option<Vec<String>>,
        opt_level: i32,
    ) -> PyResult<Self> {
        let source = Source::Text {
            name: name.to_owned(),
            source: source.to_owned(),
        };
        let factory = build_factory(py, source, double, search_paths, backend, args, opt_level)?;
        Ok(Self { factory })
    }

    /// Compiles the program in the file at `path`, named after its stem.
    /// `import(...)` searches `search_paths`, then the installed libraries,
    /// then the file's own directory. Other arguments as for `Factory(...)`.
    #[staticmethod]
    #[pyo3(signature = (path, double = false, search_paths = None, backend = "interp", args = None, opt_level = 0))]
    fn from_file(
        py: Python<'_>,
        path: PathBuf,
        double: bool,
        search_paths: Option<Vec<String>>,
        backend: &str,
        args: Option<Vec<String>>,
        opt_level: i32,
    ) -> PyResult<Self> {
        let source = Source::File(path);
        let factory = build_factory(py, source, double, search_paths, backend, args, opt_level)?;
        Ok(Self { factory })
    }

    /// A new instance, initialised at `sample_rate` and ready to compute.
    #[pyo3(signature = (sample_rate = 48000))]
    fn create_dsp_instance(&self, sample_rate: i32) -> PyResult<Dsp> {
        check_sample_rate(sample_rate)?;
        let dsp = self
            .factory
            .create_dsp_instance(sample_rate)
            .map_err(py_err)?;
        Ok(Dsp::new(dsp))
    }

    /// The program name: the `name` argument.
    #[getter]
    fn name(&self) -> &str {
        self.factory.get_name()
    }

    /// `"interp"` or `"cranelift"`.
    #[getter]
    fn backend(&self) -> String {
        self.factory.backend().to_string()
    }

    /// `"float"` (`f32`) or `"double"` (`f64`).
    #[getter]
    fn precision(&self) -> &'static str {
        precision_name(self.factory.precision())
    }

    /// The JSON description of the program, its UI and metadata, as the C
    /// API's `getDSPFactoryJSON` returns it.
    fn get_json(&self) -> String {
        self.factory.get_json()
    }

    fn __repr__(&self) -> String {
        format!(
            "Factory(name={:?}, backend={:?}, precision={:?})",
            self.name(),
            self.backend(),
            self.precision()
        )
    }
}

/// Compile a Faust `.dsp` source string into a runnable [`Dsp`] handle:
/// `Factory(...).create_dsp_instance(sample_rate)`.
///
/// `backend` is `"interp"` (the bytecode interpreter, the default) or
/// `"cranelift"` (native code through the Cranelift JIT). Cranelift does not
/// yet lower every program; one it cannot run raises `InstantiateError`.
///
/// Set `double=True` for double-precision (`f64`) DSP; the default is single
/// precision (`f32`).
///
/// `search_paths` is an optional list of directories in which to resolve
/// `import("...")` directives (e.g. a directory containing the Faust standard
/// libraries so `import("stdfaust.lib")` works). Directories listed in the
/// `FAUST_LIB_PATH` environment variable are appended automatically.
///
/// `args` are further compiler flags, passed verbatim (e.g. `["-vec", "-vs",
/// "16"]`); precision flags are refused in favour of `double=`. `opt_level`
/// is the Cranelift optimisation level, 0 to 3; the interpreter ignores it.
#[pyfunction]
#[pyo3(signature = (source, name = "FaustDSP", sample_rate = 48000, double = false, search_paths = None, backend = "interp", args = None, opt_level = 0))]
#[allow(clippy::too_many_arguments)]
fn compile(
    py: Python<'_>,
    source: &str,
    name: &str,
    sample_rate: i32,
    double: bool,
    search_paths: Option<Vec<String>>,
    backend: &str,
    args: Option<Vec<String>>,
    opt_level: i32,
) -> PyResult<Dsp> {
    check_sample_rate(sample_rate)?;
    Factory::new(
        py,
        source,
        name,
        double,
        search_paths,
        backend,
        args,
        opt_level,
    )?
    .create_dsp_instance(sample_rate)
}

/// Compile the `.dsp` file at `path` into a runnable [`Dsp`] handle:
/// `Factory.from_file(...).create_dsp_instance(sample_rate)`. Arguments as
/// for [`compile`], without `name`: the program is named after the file stem.
#[pyfunction]
#[pyo3(signature = (path, sample_rate = 48000, double = false, search_paths = None, backend = "interp", args = None, opt_level = 0))]
#[allow(clippy::too_many_arguments)]
fn compile_file(
    py: Python<'_>,
    path: PathBuf,
    sample_rate: i32,
    double: bool,
    search_paths: Option<Vec<String>>,
    backend: &str,
    args: Option<Vec<String>>,
    opt_level: i32,
) -> PyResult<Dsp> {
    check_sample_rate(sample_rate)?;
    Factory::from_file(py, path, double, search_paths, backend, args, opt_level)?
        .create_dsp_instance(sample_rate)
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
    m.add_function(wrap_pyfunction!(compile_file, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<Factory>()?;
    m.add_class::<Dsp>()?;
    m.add_class::<Param>()?;
    let py = m.py();
    m.add("FaustError", py.get_type::<FaustError>())?;
    m.add("CompileError", py.get_type::<CompileError>())?;
    m.add("InstantiateError", py.get_type::<InstantiateError>())?;
    m.add("UnknownParamError", py.get_type::<UnknownParamError>())?;
    m.add("ReadOnlyParamError", py.get_type::<ReadOnlyParamError>())?;
    m.add("BuffersError", py.get_type::<BuffersError>())?;
    Ok(())
}
