//! FIR module emission for the signal->FIR fast-lane.
//!
//! Step 2A..2G lowers an executable fast-lane slice:
//! - `SIGINPUT`, integer/real constants,
//! - `SIGBINOP` (arithmetic/comparison/bitwise subset),
//! - `SIGPOW`/`SIGMIN`/`SIGMAX`,
//! - core unary math nodes (`sin/cos/tan/exp/log/log10/sqrt/abs`),
//! - `SIGDELAY1`/`SIGDELAY`/`SIGPREFIX`,
//! - `SIGSELECT2`, `SIGINTCAST`/`SIGFLOATCAST`/`SIGBITCAST`,
//! - `SIGPROJ`/`SIGREC` (real lowering for canonical recursion groups in de
//!   Bruijn or symbolic form).
//! - `SIGWAVEFORM`/`SIGRDTBL`/`SIGWRTBL` for direct waveform tables.
//! - `SIGOUTPUT` passthrough nodes.
//! - sectioned FIR module assembly (`metadata`, `instanceConstants`,
//!   `instanceResetUserInterface`, `instanceClear`, `buildUserInterface`, `compute`).
//!
//! Section placement policy (Step 3B):
//! - `instanceConstants`: table initialization and compile-time constants.
//! - `instanceResetUserInterface`: UI zone reset values.
//! - `instanceClear`: runtime signal state reset values (delay/rec state).
//!
//! Integer policy:
//! - `SIGINT`/`SIGINTCAST` and integer bitwise operations lower to FIR `Int32`
//!   nodes/types for C++ parity in the active fast-lane.
//!
//! Type duality policy (internal vs external):
//! - **Internal real type** (`real_ty`, default `FirType::Float32`): used for
//!   all internal DSP computation — state variables, arithmetic results, math
//!   call signatures, waveform table element types, and real constants.
//!   Configurable at module build time via [`super::RealType`].
//! - **External type** (`FirType::FaustFloat`): used exclusively for the
//!   `FAUSTFLOAT**` audio buffer parameters in `compute`, and for UI zone
//!   struct variables (sliders, bargraphs, buttons) that are read/written by
//!   the host application.
//! - Implicit casts are emitted at every boundary:
//!   - input sample load: `FaustFloat → real_ty`,
//!   - output sample store: `real_ty → FaustFloat`,
//!   - UI zone read (for computation): `FaustFloat → real_ty`,
//!   - bargraph zone write (from computation): `real_ty → FaustFloat`.
//!
//! Other signal families still return typed `FRS-SFIR-*` errors.

use std::collections::{HashMap, HashSet};

use fir::{
    AccessType, BargraphType, ButtonType, FirBinOp, FirBuilder, FirId, FirMatch, FirMathOp,
    FirStore, FirType, NamedType, SliderRange, SliderType, UiBoxType, match_fir,
};
use signals::{BinOp, SigId, SigMatch, dump_sig_readable, match_sig};
use tlib::{NodeKind, TreeArena, match_sym_rec, match_sym_ref, tree_to_int};

use super::SignalFirOutput;
use super::error::{SignalFirError, SignalFirErrorCode};
use super::planner::SignalFirPlan;

/// Deterministic prototype emission order for math helper functions.
///
/// Keeping this order stable avoids noisy golden diffs in generated FIR/C/C++.
const MATH_PROTO_ORDER: &[FirMathOp] = &[
    FirMathOp::Pow,
    FirMathOp::Min,
    FirMathOp::Max,
    FirMathOp::Sin,
    FirMathOp::Cos,
    FirMathOp::Acos,
    FirMathOp::Asin,
    FirMathOp::Atan,
    FirMathOp::Atan2,
    FirMathOp::Tan,
    FirMathOp::Exp,
    FirMathOp::Log,
    FirMathOp::Log10,
    FirMathOp::Sqrt,
    FirMathOp::Abs,
    FirMathOp::Fmod,
    FirMathOp::Remainder,
    FirMathOp::Floor,
    FirMathOp::Ceil,
    FirMathOp::Rint,
    FirMathOp::Round,
];

/// Emits a FIR module from validated planning data and propagated signals.
///
/// This is the main fast-lane lowering boundary: callers provide already
/// propagated signals plus a checked [`SignalFirPlan`], and receive a complete
/// FIR module with Faust lifecycle sections assembled in deterministic order.
pub fn build_module(
    plan: &SignalFirPlan,
    module_name: &str,
    arena: &TreeArena,
    signals: &[SigId],
    real_ty: FirType,
) -> Result<SignalFirOutput, SignalFirError> {
    let mut lower = SignalToFirLower::new(arena, plan.num_inputs, real_ty);
    let dsp_arg_type = FirType::Ptr(Box::new(FirType::Obj));
    let dsp_arg = NamedType {
        name: "dsp".to_string(),
        typ: dsp_arg_type.clone(),
    };

    {
        let mut b = FirBuilder::new(&mut lower.store);
        lower
            .control_statements
            .push(b.label("signal_fir_fastlane_step2a: executable base slice"));
        lower.control_statements.push(b.label(format!(
            "io: inputs={} outputs={}",
            plan.num_inputs, plan.num_outputs
        )));
        lower
            .control_statements
            .push(b.label(format!("signals: {}", plan.signal_count)));
    }

    for (signal_index, sig) in signals.iter().enumerate() {
        let mut value = lower.lower_signal(*sig)?;
        if signal_index < plan.num_outputs {
            // Internal real type → external FaustFloat boundary at output store.
            // Internal values are always Float32/Float64, never FaustFloat, so
            // this cast is always emitted. The check guards against future cases
            // where the value might already carry the external type.
            let needs_output_cast = lower.store.value_type(value) != Some(FirType::FaustFloat);
            let mut b = FirBuilder::new(&mut lower.store);
            if needs_output_cast {
                value = b.cast(FirType::FaustFloat, value);
            }
            let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
            lower.sample_statements.push(b.store_table(
                format!("output{signal_index}"),
                AccessType::Stack,
                i0,
                value,
            ));
        } else {
            let mut b = FirBuilder::new(&mut lower.store);
            lower.sample_statements.push(b.drop_(value));
        }
    }
    for index in 0..plan.num_outputs {
        let mut b = FirBuilder::new(&mut lower.store);
        let chan = b.int32(i32::try_from(index).expect("validated output index fits i32"));
        let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let load_chan_ptr = b.load_table("outputs", AccessType::FunArgs, chan, ptr_ty.clone());
        lower.control_statements.push(b.declare_var(
            format!("output{index}"),
            ptr_ty,
            AccessType::Stack,
            Some(load_chan_ptr),
        ));
    }
    lower
        .sample_statements
        .extend(lower.compute_updates.iter().copied());

    let metadata_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&[])
    };
    let metadata_args = [
        dsp_arg.clone(),
        NamedType {
            name: "m".to_string(),
            typ: FirType::Meta,
        },
    ];
    let metadata = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "metadata",
            FirType::Fun {
                args: vec![dsp_arg_type.clone(), FirType::Meta],
                ret: Box::new(FirType::Void),
            },
            &metadata_args,
            Some(metadata_body),
            false,
        )
    };

    let constants_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.constants_statements)
    };
    let constants_args = [
        dsp_arg.clone(),
        NamedType {
            name: "sample_rate".to_string(),
            typ: FirType::Int32,
        },
    ];
    let instance_constants = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceConstants",
            FirType::Fun {
                args: vec![dsp_arg_type.clone(), FirType::Int32],
                ret: Box::new(FirType::Void),
            },
            &constants_args,
            Some(constants_body),
            false,
        )
    };

    let reset_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.reset_statements)
    };
    let instance_reset_ui = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceResetUserInterface",
            FirType::Fun {
                args: vec![dsp_arg_type.clone()],
                ret: Box::new(FirType::Void),
            },
            std::slice::from_ref(&dsp_arg),
            Some(reset_body),
            false,
        )
    };

    let clear_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.clear_statements)
    };
    let instance_clear = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceClear",
            FirType::Fun {
                args: vec![dsp_arg_type.clone()],
                ret: Box::new(FirType::Void),
            },
            std::slice::from_ref(&dsp_arg),
            Some(clear_body),
            false,
        )
    };

    let ui_statements =
        maybe_wrap_ui_in_root_group(&mut lower.store, module_name, &lower.ui_statements);
    let ui_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&ui_statements)
    };
    let build_ui_args = [
        dsp_arg.clone(),
        NamedType {
            name: "ui_interface".to_string(),
            typ: FirType::UI,
        },
    ];
    let build_ui = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "buildUserInterface",
            FirType::Fun {
                args: vec![dsp_arg_type.clone(), FirType::UI],
                ret: Box::new(FirType::Void),
            },
            &build_ui_args,
            Some(ui_body),
            false,
        )
    };

    let compute_statements = {
        let mut all = Vec::new();
        all.extend(lower.control_statements.iter().copied());
        if !lower.sample_statements.is_empty() {
            let sample_loop = {
                let mut b = FirBuilder::new(&mut lower.store);
                let upper = b.load_var("count", AccessType::FunArgs, FirType::Int32);
                let body = b.block(&lower.sample_statements);
                b.simple_for_loop("i0", upper, body, false)
            };
            all.push(sample_loop);
        }
        all
    };
    let compute_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&compute_statements)
    };
    let compute_args = [
        dsp_arg.clone(),
        NamedType {
            name: "count".to_string(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    dsp_arg_type,
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        )
    };

    // Math function prototypes use the internal real type for both arguments and
    // return value: `sin`, `cos`, `pow`, etc. operate on internal-precision samples.
    let math_real_ty = lower.real_ty();
    let mut math_prototypes = Vec::new();
    for op in MATH_PROTO_ORDER {
        if !lower.used_math_ops.contains(op) {
            continue;
        }
        let arity = match op {
            FirMathOp::Pow
            | FirMathOp::Min
            | FirMathOp::Max
            | FirMathOp::Atan2
            | FirMathOp::Fmod
            | FirMathOp::Remainder => 2,
            _ => 1,
        };
        let proto_args: Vec<NamedType> = (0..arity)
            .map(|i| NamedType {
                name: format!("arg{i}"),
                typ: math_real_ty.clone(),
            })
            .collect();
        let proto = {
            let mut b = FirBuilder::new(&mut lower.store);
            b.declare_fun(
                op.symbol(),
                FirType::Fun {
                    args: vec![math_real_ty.clone(); arity],
                    ret: Box::new(math_real_ty.clone()),
                },
                &proto_args,
                None,
                false,
            )
        };
        math_prototypes.push(proto);
    }

    let functions = {
        let mut b = FirBuilder::new(&mut lower.store);
        let function_items = [
            metadata,
            instance_constants,
            instance_reset_ui,
            instance_clear,
            build_ui,
            compute,
        ];
        b.block(&function_items)
    };
    let dsp_struct = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.struct_declarations)
    };
    let globals = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&math_prototypes)
    };
    let module: FirId = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.module(
            plan.num_inputs,
            plan.num_outputs,
            module_name,
            dsp_struct,
            globals,
            functions,
        )
    };

    Ok(SignalFirOutput {
        store: lower.store,
        module,
    })
}

/// Wraps top-level UI widgets in an implicit root vertical box when needed.
///
/// Parity policy:
/// - if explicit groups (`OpenBox`/`CloseBox`) are already present, keep UI as-is;
/// - if widgets exist but no group exists, inject:
///   `openVerticalBox(module_name) ... closeBox`.
///
/// This mirrors Faust behavior where a root group is synthesized to keep UI
/// hierarchy valid for consumers expecting balanced open/close group events.
fn maybe_wrap_ui_in_root_group(
    store: &mut FirStore,
    module_name: &str,
    ui_statements: &[FirId],
) -> Vec<FirId> {
    if ui_statements.is_empty() {
        return Vec::new();
    }

    let mut has_group = false;
    let mut has_widget = false;
    for stmt in ui_statements {
        match match_fir(store, *stmt) {
            FirMatch::OpenBox { .. } | FirMatch::CloseBox => has_group = true,
            FirMatch::AddButton { .. }
            | FirMatch::AddSlider { .. }
            | FirMatch::AddBargraph { .. }
            | FirMatch::AddSoundfile { .. } => has_widget = true,
            _ => {}
        }
    }

    if has_group || !has_widget {
        return ui_statements.to_vec();
    }

    let mut wrapped = Vec::with_capacity(ui_statements.len() + 2);
    let mut b = FirBuilder::new(store);
    wrapped.push(b.open_box(UiBoxType::Vertical, module_name));
    wrapped.extend(ui_statements.iter().copied());
    wrapped.push(b.close_box());
    wrapped
}

/// Stateful lowering engine from propagated signals to FIR.
///
/// Design notes:
/// - memoizes lowered signal nodes in [`Self::cache`] for DAG sharing;
/// - splits statements by lifecycle section (`constants/reset/clear/compute`);
/// - tracks emitted state/UI/table declarations to keep output deterministic and
///   avoid duplicate declarations.
///
/// This struct is deliberately stateful instead of purely recursive because the
/// target FIR module has to be assembled from several side channels at once:
/// value expressions, persistent state declarations, UI declarations, waveform
/// tables, and scheduled compute-time updates.
struct SignalToFirLower<'a> {
    arena: &'a TreeArena,
    num_inputs: usize,
    /// Internal DSP computation type (e.g. `Float32` or `Float64`).
    ///
    /// This is the type used for all internal signal computation: arithmetic
    /// results, state variable declarations, math call return types, waveform
    /// table element types, and real constants.
    ///
    /// **Never** used for external interface points: audio buffer samples and
    /// UI zone variables always use `FirType::FaustFloat`.
    real_ty: FirType,
    store: FirStore,
    cache: HashMap<SigId, FirId>,
    struct_declarations: Vec<FirId>,
    constants_statements: Vec<FirId>,
    reset_statements: Vec<FirId>,
    clear_statements: Vec<FirId>,
    control_statements: Vec<FirId>,
    sample_statements: Vec<FirId>,
    compute_updates: Vec<FirId>,
    state_name_by_node: HashMap<SigId, String>,
    scheduled_state_updates: HashSet<SigId>,
    recursion_stack: Vec<String>,
    recursion_vars: Vec<SigId>,
    ui_controls: HashMap<SigId, String>,
    soundfiles: HashMap<SigId, String>,
    waveform_tables: HashMap<SigId, String>,
    waveform_table_len: HashMap<SigId, usize>,
    ui_statements: Vec<FirId>,
    named_struct_vars: HashSet<String>,
    reset_init_seen: HashSet<String>,
    clear_init_seen: HashSet<String>,
    input_ptr_aliases: HashMap<usize, String>,
    used_math_ops: HashSet<FirMathOp>,
}

impl<'a> SignalToFirLower<'a> {
    /// Creates one fresh lowering state for a module build.
    ///
    /// Each `build_module` call gets its own lowerer so caches and section
    /// accumulators cannot leak across compilations.
    fn new(arena: &'a TreeArena, num_inputs: usize, real_ty: FirType) -> Self {
        Self {
            arena,
            num_inputs,
            real_ty,
            store: FirStore::new(),
            cache: HashMap::new(),
            struct_declarations: Vec::new(),
            constants_statements: Vec::new(),
            reset_statements: Vec::new(),
            clear_statements: Vec::new(),
            control_statements: Vec::new(),
            sample_statements: Vec::new(),
            compute_updates: Vec::new(),
            state_name_by_node: HashMap::new(),
            scheduled_state_updates: HashSet::new(),
            recursion_stack: Vec::new(),
            recursion_vars: Vec::new(),
            ui_controls: HashMap::new(),
            soundfiles: HashMap::new(),
            waveform_tables: HashMap::new(),
            waveform_table_len: HashMap::new(),
            ui_statements: Vec::new(),
            named_struct_vars: HashSet::new(),
            reset_init_seen: HashSet::new(),
            clear_init_seen: HashSet::new(),
            input_ptr_aliases: HashMap::new(),
            used_math_ops: HashSet::new(),
        }
    }

    /// Returns a clone of the internal real computation type.
    ///
    /// Use this whenever a FIR node must carry the internal scalar precision
    /// (arithmetic result, state slot, math call, real constant, …).
    /// For external interface points (audio buffer samples, UI zone variables)
    /// use `FirType::FaustFloat` directly instead.
    fn real_ty(&self) -> FirType {
        self.real_ty.clone()
    }

    /// Lowers one signal node to a FIR value expression.
    ///
    /// This function is the central dispatcher over [`signals::SigMatch`].
    ///
    /// Successful lowering may also append statements to lifecycle sections as a
    /// side effect. For example, a returned FIR expression for a delay node is
    /// coupled with state declarations and deferred update stores recorded in
    /// [`Self::clear_statements`] / [`Self::compute_updates`].
    ///
    /// Unsupported families return typed `FRS-SFIR-*` errors.
    fn lower_signal(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        if let Some(id) = self.cache.get(&sig).copied() {
            return Ok(id);
        }

        let lowered = match match_sig(self.arena, sig) {
            SigMatch::Int(value) => self.lower_int32_const(value),
            // Real constant: emitted at internal precision (Float32 or Float64).
            SigMatch::Real(value) => self.float_const(value),
            SigMatch::Input(index) => self.lower_input(index)?,
            SigMatch::Output(_, inner) => self.lower_signal(inner)?,
            SigMatch::Delay1(value) => {
                let init = self.float_const(0.0);
                self.lower_delay_state(sig, value, init)?
            }
            SigMatch::Delay(value, amount) => self.lower_delay(sig, value, amount)?,
            SigMatch::Prefix(init_sig, value) => {
                let init = self.initial_state_from_signal(init_sig);
                self.lower_delay_state(sig, value, init)?
            }
            SigMatch::IntCast(value) => self.lower_cast(FirType::Int32, value)?,
            // BitCast and FloatCast convert to the internal real type, not to
            // FaustFloat: they are integer↔float reinterpretation/coercion
            // operations used in internal DSP computation.
            SigMatch::BitCast(value) => self.lower_bitcast(self.real_ty(), value)?,
            SigMatch::FloatCast(value) => self.lower_cast(self.real_ty(), value)?,
            SigMatch::Select2(cond, then_value, else_value) => {
                self.lower_select2(cond, then_value, else_value)?
            }
            SigMatch::Proj(index, group) => self.lower_proj(sig, index, group)?,
            SigMatch::Rec(body) => self.lower_signal(body)?,
            SigMatch::BinOp(op, lhs, rhs) => self.lower_binop(op, lhs, rhs)?,
            SigMatch::Pow(lhs, rhs) => self.lower_math2(FirMathOp::Pow, lhs, rhs)?,
            SigMatch::Min(lhs, rhs) => self.lower_math2(FirMathOp::Min, lhs, rhs)?,
            SigMatch::Max(lhs, rhs) => self.lower_math2(FirMathOp::Max, lhs, rhs)?,
            SigMatch::Sin(value) => self.lower_math1(FirMathOp::Sin, value)?,
            SigMatch::Cos(value) => self.lower_math1(FirMathOp::Cos, value)?,
            SigMatch::Acos(value) => self.lower_math1(FirMathOp::Acos, value)?,
            SigMatch::Asin(value) => self.lower_math1(FirMathOp::Asin, value)?,
            SigMatch::Atan(value) => self.lower_math1(FirMathOp::Atan, value)?,
            SigMatch::Atan2(lhs, rhs) => self.lower_math2(FirMathOp::Atan2, lhs, rhs)?,
            SigMatch::Tan(value) => self.lower_math1(FirMathOp::Tan, value)?,
            SigMatch::Exp(value) => self.lower_math1(FirMathOp::Exp, value)?,
            SigMatch::Log(value) => self.lower_math1(FirMathOp::Log, value)?,
            SigMatch::Log10(value) => self.lower_math1(FirMathOp::Log10, value)?,
            SigMatch::Sqrt(value) => self.lower_math1(FirMathOp::Sqrt, value)?,
            SigMatch::Abs(value) => self.lower_math1(FirMathOp::Abs, value)?,
            SigMatch::Fmod(lhs, rhs) => self.lower_math2(FirMathOp::Fmod, lhs, rhs)?,
            SigMatch::Remainder(lhs, rhs) => self.lower_math2(FirMathOp::Remainder, lhs, rhs)?,
            SigMatch::Floor(value) => self.lower_math1(FirMathOp::Floor, value)?,
            SigMatch::Ceil(value) => self.lower_math1(FirMathOp::Ceil, value)?,
            SigMatch::Rint(value) => self.lower_math1(FirMathOp::Rint, value)?,
            SigMatch::Round(value) => self.lower_math1(FirMathOp::Round, value)?,
            SigMatch::Lowest(value) => self.lower_signal(value)?,
            SigMatch::Highest(value) => self.lower_signal(value)?,
            SigMatch::RdTbl(tbl, ridx) => self.lower_rdtbl(sig, tbl, ridx)?,
            SigMatch::WrTbl(size, generator, widx, wsig) => {
                self.lower_wrtbl(sig, size, generator, widx, wsig)?
            }
            SigMatch::Waveform(values) => self.lower_waveform(sig, values)?,
            SigMatch::Button(label) => self.lower_button(sig, label, ButtonType::Button),
            SigMatch::Checkbox(label) => self.lower_button(sig, label, ButtonType::Checkbox),
            SigMatch::VSlider(label, init, min, max, step) => {
                self.lower_slider(sig, [label, init, min, max, step], SliderType::Vertical)?
            }
            SigMatch::HSlider(label, init, min, max, step) => {
                self.lower_slider(sig, [label, init, min, max, step], SliderType::Horizontal)?
            }
            SigMatch::NumEntry(label, init, min, max, step) => {
                self.lower_slider(sig, [label, init, min, max, step], SliderType::NumEntry)?
            }
            SigMatch::VBargraph(label, min, max, value) => {
                self.lower_bargraph(sig, label, min, max, value, BargraphType::Vertical)?
            }
            SigMatch::HBargraph(label, min, max, value) => {
                self.lower_bargraph(sig, label, min, max, value, BargraphType::Horizontal)?
            }
            SigMatch::Attach(lhs, rhs) => {
                let _ = self.lower_signal(rhs)?;
                self.lower_signal(lhs)?
            }
            SigMatch::Enable(lhs, rhs) => {
                let zero = self.float_const(0.0);
                let lhs = self.lower_signal(lhs)?;
                let cond = self.lower_signal(rhs)?;
                let real_ty = self.real_ty();
                let mut b = FirBuilder::new(&mut self.store);
                b.select2(cond, lhs, zero, real_ty)
            }
            SigMatch::Control(lhs, rhs) => {
                let _ = self.lower_signal(rhs)?;
                self.lower_signal(lhs)?
            }
            SigMatch::Soundfile(label) => self.lower_soundfile(sig, label),
            other => {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "unsupported signal node in Step 2C: {other:?} (expr={})",
                        dump_sig_readable(self.arena, sig)
                    ),
                ));
            }
        };

        self.cache.insert(sig, lowered);
        Ok(lowered)
    }

    /// Lowers one input signal by materializing channel-pointer aliases once
    /// and generating a per-sample table load (`inputN[i0]`).
    fn lower_input(&mut self, index: i32) -> Result<FirId, SignalFirError> {
        let index = usize::try_from(index).map_err(|_| {
            SignalFirError::new(
                SignalFirErrorCode::InputIndexOutOfRange,
                "input index conversion overflow",
            )
        })?;
        if index >= self.num_inputs {
            return Err(SignalFirError::new(
                SignalFirErrorCode::InputIndexOutOfRange,
                format!(
                    "input index {index} is out of range for num_inputs={}",
                    self.num_inputs
                ),
            ));
        }

        let alias = if let Some(alias) = self.input_ptr_aliases.get(&index) {
            alias.clone()
        } else {
            let alias = format!("input{index}");
            let mut b = FirBuilder::new(&mut self.store);
            let chan = b.int32(i32::try_from(index).expect("validated input index fits i32"));
            let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
            let load_chan_ptr = b.load_table("inputs", AccessType::FunArgs, chan, ptr_ty.clone());
            self.control_statements.push(b.declare_var(
                alias.clone(),
                ptr_ty,
                AccessType::Stack,
                Some(load_chan_ptr),
            ));
            self.input_ptr_aliases.insert(index, alias.clone());
            alias
        };

        // Load the sample from the external FAUSTFLOAT buffer, then cast to the
        // internal real type so all downstream computation uses real_ty.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let raw = b.load_table(alias, AccessType::Stack, i0, FirType::FaustFloat);
        Ok(b.cast(real_ty, raw))
    }

    /// Lowers general `SIGDELAY` currently restricted to integer amount `1`.
    ///
    /// Non-unit delays are explicitly rejected in the current fast-lane slice.
    fn lower_delay(
        &mut self,
        node: SigId,
        value: SigId,
        amount: SigId,
    ) -> Result<FirId, SignalFirError> {
        match match_sig(self.arena, amount) {
            SigMatch::Int(1) => {
                let init = self.float_const(0.0);
                self.lower_delay_state(node, value, init)
            }
            SigMatch::Int(other) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGDELAY amount {other} unsupported in Step 2C (only 1)"),
            )),
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGDELAY amount must be integer in Step 2C",
            )),
        }
    }

    /// Lowers one single-sample state edge (`delay1`/`prefix`) as:
    /// `out = load(state); update(state, next)` with update deferred to the
    /// compute-loop update list.
    fn lower_delay_state(
        &mut self,
        node: SigId,
        value: SigId,
        init: FirId,
    ) -> Result<FirId, SignalFirError> {
        let name = self.ensure_state_slot(node, init);
        let out = {
            // State slot holds internal-precision samples.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            b.load_var(name.clone(), AccessType::Struct, real_ty)
        };
        if self.scheduled_state_updates.insert(node) {
            let next = self.lower_signal(value)?;
            let mut b = FirBuilder::new(&mut self.store);
            self.compute_updates
                .push(b.store_var(name, AccessType::Struct, next));
        }
        Ok(out)
    }

    /// Ensures one struct state slot exists for `node`, creating declaration
    /// and `instanceClear` initialization on first use.
    fn ensure_state_slot(&mut self, node: SigId, init: FirId) -> String {
        if let Some(name) = self.state_name_by_node.get(&node) {
            return name.clone();
        }
        let name = format!("state_n{}", node.as_u32());
        // State slots are internal-precision variables, not FaustFloat.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(name.clone(), real_ty, AccessType::Struct, None);
        self.struct_declarations.push(dec);
        self.register_clear_init(name.clone(), init);
        self.state_name_by_node.insert(node, name.clone());
        name
    }

    /// Emits one floating-point constant at the internal real precision.
    ///
    /// Uses `Float32` or `Float64` depending on `real_ty`.  Never emits
    /// `FaustFloat` — that type is reserved for external interface points.
    fn float_const(&mut self, value: f64) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        match self.real_ty {
            FirType::Float64 => b.float64(value),
            _ => b.float32(value as f32),
        }
    }

    /// Derives an initial state value from a signal if constant, otherwise `0`.
    fn initial_state_from_signal(&mut self, sig: SigId) -> FirId {
        match match_sig(self.arena, sig) {
            SigMatch::Int(v) => self.lower_int32_const(v),
            SigMatch::Real(v) => self.float_const(v),
            _ => self.float_const(0.0),
        }
    }

    /// Emits one `Int32` FIR constant.
    fn lower_int32_const(&mut self, value: i32) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        b.int32(value)
    }

    /// Lowers button/checkbox UI controls as zone-backed struct variables.
    fn lower_button(&mut self, node: SigId, label: SigId, typ: ButtonType) -> FirId {
        if let Some(var) = self.ui_controls.get(&node).cloned() {
            // UI zone variable is FaustFloat (external); cast to real_ty for computation.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
            return b.cast(real_ty, load);
        }
        let var = self.ui_control_var_name(
            node,
            match typ {
                ButtonType::Button => "fButton",
                ButtonType::Checkbox => "fCheckbox",
            },
        );
        // UI zone initializer: use internal real precision so that the constant
        // type matches `real_ty` (Float32 or Float64 with `--double`).
        let init = self.float_const(0.0);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        let label = self.label_text(label);
        {
            let mut b = FirBuilder::new(&mut self.store);
            self.ui_statements
                .push(b.add_button(typ, label, var.clone()));
        }
        self.ui_controls.insert(node, var.clone());
        // Load the FaustFloat zone and cast to internal real type for computation.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
        b.cast(real_ty, load)
    }

    /// Lowers slider-style UI controls and records metadata in
    /// `buildUserInterface`.
    fn lower_slider(
        &mut self,
        node: SigId,
        params: [SigId; 5],
        typ: SliderType,
    ) -> Result<FirId, SignalFirError> {
        let [label, init, min, max, step] = params;
        if let Some(var) = self.ui_controls.get(&node).cloned() {
            // UI zone variable is FaustFloat (external); cast to real_ty for computation.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
            return Ok(b.cast(real_ty, load));
        }
        let var = self.ui_control_var_name(
            node,
            match typ {
                SliderType::Horizontal => "fHslider",
                SliderType::Vertical => "fVslider",
                SliderType::NumEntry => "fEntry",
            },
        );
        let init_v = self.constant_f64(init).unwrap_or(0.0);
        let min_v = self.constant_f64(min).unwrap_or(0.0);
        let max_v = self.constant_f64(max).unwrap_or(1.0);
        let step_v = self.constant_f64(step).unwrap_or(0.01);
        // UI zone initializer: use internal real precision so that the constant
        // type matches `real_ty` (Float32 or Float64 with `--double`).
        // The range metadata stays f64 for precision.
        let init_id = self.float_const(init_v);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init_id));
        let label = self.label_text(label);
        let range = SliderRange {
            init: init_v,
            lo: min_v,
            hi: max_v,
            step: step_v,
        };
        {
            let mut b = FirBuilder::new(&mut self.store);
            self.ui_statements
                .push(b.add_slider(typ, label, var.clone(), range));
        }
        self.ui_controls.insert(node, var.clone());
        // Load the FaustFloat zone and cast to internal real type for computation.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
        Ok(b.cast(real_ty, load))
    }

    /// Lowers bargraph UI nodes by creating UI descriptors and storing incoming
    /// runtime value in a dedicated control zone.
    fn lower_bargraph(
        &mut self,
        node: SigId,
        label: SigId,
        min: SigId,
        max: SigId,
        value: SigId,
        typ: BargraphType,
    ) -> Result<FirId, SignalFirError> {
        if !self.ui_controls.contains_key(&node) {
            let var = self.ui_control_var_name(
                node,
                match typ {
                    BargraphType::Horizontal => "fHbargraph",
                    BargraphType::Vertical => "fVbargraph",
                },
            );
            // Bargraph zone is FaustFloat (the host reads it); initializer uses
            // internal real precision so the constant type matches `real_ty`.
            let init = self.float_const(0.0);
            self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
            let label = self.label_text(label);
            let min_v = self.constant_f64(min).unwrap_or(0.0);
            let max_v = self.constant_f64(max).unwrap_or(1.0);
            let mut b = FirBuilder::new(&mut self.store);
            self.ui_statements
                .push(b.add_bargraph(typ, label, var.clone(), min_v, max_v));
            self.ui_controls.insert(node, var);
        }
        // The incoming signal value is computed at internal real precision; cast
        // it to FaustFloat before writing to the external zone variable.
        let value = self.lower_signal(value)?;
        let var = self
            .ui_controls
            .get(&node)
            .cloned()
            .expect("bargraph variable should exist after declaration");
        let mut b = FirBuilder::new(&mut self.store);
        let faust_value = b.cast(FirType::FaustFloat, value);
        self.sample_statements
            .push(b.store_var(var, AccessType::Struct, faust_value));
        Ok(value)
    }

    /// Lowers a soundfile declaration into UI-only registration and an opaque
    /// struct-backed runtime handle.
    fn lower_soundfile(&mut self, node: SigId, label: SigId) -> FirId {
        if let Some(var) = self.soundfiles.get(&node).cloned() {
            let mut b = FirBuilder::new(&mut self.store);
            return b.load_var(var, AccessType::Struct, FirType::Sound);
        }
        let var = format!("fSound{}", node.as_u32());
        self.ensure_named_struct_var(&var, FirType::Sound, None);
        let label = self.label_text(label);
        let mut b = FirBuilder::new(&mut self.store);
        self.ui_statements.push(b.add_soundfile(label, var.clone()));
        self.soundfiles.insert(node, var.clone());
        b.load_var(var, AccessType::Struct, FirType::Sound)
    }

    /// Lowers waveform literals into constant FIR tables and returns a pointer
    /// alias to the declared table.
    fn lower_waveform(&mut self, node: SigId, values: &[SigId]) -> Result<FirId, SignalFirError> {
        let table_name = self.ensure_waveform_table(node, values)?;
        let index = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(0)
        };
        // Waveform table elements are internal-precision.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(table_name, AccessType::Struct, index, real_ty))
    }

    /// Lowers one table read by resolving the table producer and normalizing
    /// the runtime read index according to table length.
    fn lower_rdtbl(
        &mut self,
        node: SigId,
        tbl: SigId,
        ridx: SigId,
    ) -> Result<FirId, SignalFirError> {
        // Keep C++ `compileSigRDTbl` evaluation order: evaluate table first so
        // pending `wrtbl` side-effects are emitted before read access.
        let _ = self.lower_signal(tbl)?;
        let (table_name, table_len) = self.resolve_table(tbl)?;
        if table_len == 0 {
            return self.unsupported_node(node, "SIGRDTBL cannot read an empty table");
        }
        let ridx = self.lower_signal(ridx)?;
        let index = self.normalized_table_index(ridx, table_len);
        // Table elements are internal-precision.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(table_name, AccessType::Struct, index, real_ty))
    }

    /// Lowers one table write producer (`SIGWRTBL`) and returns the table alias.
    ///
    /// Current scope supports deterministic constant-size tables with generator
    /// expansion handled by [`Self::expand_generator_values`].
    fn lower_wrtbl(
        &mut self,
        node: SigId,
        _size: SigId,
        generator: SigId,
        widx: SigId,
        wsig: SigId,
    ) -> Result<FirId, SignalFirError> {
        let (table_name, table_len) = self.resolve_table(node)?;
        if table_len == 0 {
            return self.unsupported_node(generator, "SIGWRTBL cannot write an empty table");
        }
        if self.arena.is_nil(widx) {
            if self.arena.is_nil(wsig) {
                return Ok(self.float_const(0.0));
            }
            return self.lower_signal(wsig);
        }
        if self.arena.is_nil(wsig) {
            return self.unsupported_node(node, "SIGWRTBL write requires wsig when widx is set");
        }
        let wsig_value = self.lower_signal(wsig)?;
        let widx = self.lower_signal(widx)?;
        let index = self.normalized_table_index(widx, table_len);
        let mut b = FirBuilder::new(&mut self.store);
        self.compute_updates
            .push(b.store_table(table_name, AccessType::Struct, index, wsig_value));
        Ok(wsig_value)
    }

    /// Resolves a table-producing signal into `(table_name, table_len)`.
    fn resolve_table(&mut self, sig: SigId) -> Result<(String, usize), SignalFirError> {
        if let Some(name) = self.waveform_tables.get(&sig).cloned() {
            let len = self.waveform_table_len.get(&sig).copied().unwrap_or(0);
            return Ok((name, len));
        }
        match match_sig(self.arena, sig) {
            SigMatch::Waveform(values) => {
                let name = self.ensure_waveform_table(sig, values)?;
                Ok((name, values.len()))
            }
            SigMatch::WrTbl(size, generator, _, _) => self.ensure_wrtbl_table(sig, size, generator),
            _ => self.unsupported_node(
                sig,
                "table access currently supports SIGWAVEFORM and SIGWRTBL forms in Step 2H",
            ),
        }
    }

    /// Ensures one waveform table declaration is emitted exactly once.
    fn ensure_waveform_table(
        &mut self,
        sig: SigId,
        values: &[SigId],
    ) -> Result<String, SignalFirError> {
        if let Some(name) = self.waveform_tables.get(&sig).cloned() {
            return Ok(name);
        }
        let mut lowered_values = Vec::with_capacity(values.len());
        for value in values {
            lowered_values.push(self.lower_signal(*value)?);
        }
        let declared_zeros = self.zero_table_values(values.len());
        let name = format!("table_n{}", sig.as_u32());
        // Waveform tables hold internal-precision samples, not FaustFloat.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(name.clone(), AccessType::Struct, real_ty, &declared_zeros);
        self.struct_declarations.push(decl);
        for (index, value) in lowered_values.iter().copied().enumerate() {
            let index = {
                let mut b = FirBuilder::new(&mut self.store);
                b.int32(i32::try_from(index).unwrap_or(i32::MAX))
            };
            let mut b = FirBuilder::new(&mut self.store);
            self.constants_statements.push(b.store_table(
                name.clone(),
                AccessType::Struct,
                index,
                value,
            ));
        }
        self.waveform_tables.insert(sig, name.clone());
        self.waveform_table_len.insert(sig, values.len());
        Ok(name)
    }

    /// Ensures one writable table declaration and initialization are emitted
    /// exactly once.
    fn ensure_wrtbl_table(
        &mut self,
        sig: SigId,
        size_sig: SigId,
        generator_sig: SigId,
    ) -> Result<(String, usize), SignalFirError> {
        let size = self.table_size_from_sig(size_sig)?;
        let generated = self.expand_generator_values(generator_sig, size)?;
        let declared_zeros = self.zero_table_values(size);
        let name = format!("table_n{}", sig.as_u32());
        // Writable DSP tables hold internal-precision samples, not FaustFloat.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(name.clone(), AccessType::Struct, real_ty, &declared_zeros);
        self.struct_declarations.push(decl);
        for (index, value) in generated.iter().copied().enumerate() {
            let index = {
                let mut b = FirBuilder::new(&mut self.store);
                b.int32(i32::try_from(index).unwrap_or(i32::MAX))
            };
            let mut b = FirBuilder::new(&mut self.store);
            self.constants_statements.push(b.store_table(
                name.clone(),
                AccessType::Struct,
                index,
                value,
            ));
        }
        self.waveform_tables.insert(sig, name.clone());
        self.waveform_table_len.insert(sig, size);
        Ok((name, size))
    }

    /// Creates a zero-filled table initializer fallback.
    fn zero_table_values(&mut self, size: usize) -> Vec<FirId> {
        let zero = self.float_const(0.0);
        vec![zero; size]
    }

    /// Evaluates table-size signal to a positive `usize`.
    fn table_size_from_sig(&self, size_sig: SigId) -> Result<usize, SignalFirError> {
        match match_sig(self.arena, size_sig) {
            SigMatch::Int(v) if v > 0 => usize::try_from(v).map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("SIGWRTBL size conversion overflow: {v}"),
                )
            }),
            SigMatch::Int(v) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGWRTBL size must be > 0, got {v}"),
            )),
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGWRTBL currently requires constant integer size in Step 2H",
            )),
        }
    }

    /// Expands a table generator signal into concrete initializer values.
    ///
    /// Only generator shapes that can be fully resolved at compile-time are
    /// accepted in the current fast-lane slice.
    fn expand_generator_values(
        &mut self,
        generator_sig: SigId,
        size: usize,
    ) -> Result<Vec<FirId>, SignalFirError> {
        let init_sig = if let SigMatch::Gen(inner) = match_sig(self.arena, generator_sig) {
            inner
        } else {
            generator_sig
        };
        match match_sig(self.arena, init_sig) {
            SigMatch::Waveform(values) => {
                if values.is_empty() {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGGEN waveform cannot be empty in Step 2H",
                    ));
                }
                let mut out = Vec::with_capacity(size);
                for index in 0..size {
                    let item = values[index % values.len()];
                    out.push(self.lower_signal(item)?);
                }
                Ok(out)
            }
            SigMatch::Int(_) | SigMatch::Real(_) => {
                let v = self.lower_signal(init_sig)?;
                Ok(vec![v; size])
            }
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGGEN table init unsupported in Step 2H (expr={})",
                    dump_sig_readable(self.arena, init_sig)
                ),
            )),
        }
    }

    /// Normalizes one table index to `[0, table_len)` with integer modulo.
    fn normalized_table_index(&mut self, index: FirId, table_len: usize) -> FirId {
        let idx_i32 = {
            let mut b = FirBuilder::new(&mut self.store);
            b.cast(FirType::Int32, index)
        };
        let size = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(i32::try_from(table_len).unwrap_or(i32::MAX))
        };
        let rem = {
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Rem, idx_i32, size, FirType::Int32)
        };
        let rem_plus_size = {
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Add, rem, size, FirType::Int32)
        };
        let mut b = FirBuilder::new(&mut self.store);
        b.binop(FirBinOp::Rem, rem_plus_size, size, FirType::Int32)
    }

    /// Declares one named struct variable once.
    fn ensure_named_struct_var(&mut self, name: &str, typ: FirType, init: Option<FirId>) {
        if self.named_struct_vars.contains(name) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(name.to_owned(), typ, AccessType::Struct, None);
        self.struct_declarations.push(dec);
        self.named_struct_vars.insert(name.to_owned());
        if let Some(init) = init {
            self.register_reset_init(name.to_owned(), init);
        }
    }

    /// Registers one reset-time assignment for UI controls (`instanceResetUserInterface`).
    fn register_reset_init(&mut self, name: String, init: FirId) {
        if !self.reset_init_seen.insert(name.clone()) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.reset_statements
            .push(b.store_var(name, AccessType::Struct, init));
    }

    /// Registers one clear-time assignment for runtime state (`instanceClear`).
    fn register_clear_init(&mut self, name: String, init: FirId) {
        if !self.clear_init_seen.insert(name.clone()) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.clear_statements
            .push(b.store_var(name, AccessType::Struct, init));
    }

    /// Helper to produce a typed unsupported-node error with readable dumped IR.
    fn unsupported_node<T>(&self, sig: SigId, detail: &str) -> Result<T, SignalFirError> {
        Err(SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("{detail} (expr={})", dump_sig_readable(self.arena, sig)),
        ))
    }

    /// Converts a label signal node to UTF-8 text fallback used by UI emit.
    fn label_text(&self, label: SigId) -> String {
        match self.arena.kind(label) {
            Some(NodeKind::Symbol(s)) => s.to_string(),
            Some(NodeKind::StringLiteral(s)) => s.to_string(),
            Some(NodeKind::Int(v)) => v.to_string(),
            Some(NodeKind::FloatBits(bits)) => f64::from_bits(*bits).to_string(),
            _ => "ui".to_owned(),
        }
    }

    /// Stable generated UI zone variable naming policy.
    fn ui_control_var_name(&self, node: SigId, prefix: &str) -> String {
        format!("{prefix}{}", node.as_u32())
    }

    /// Extracts a compile-time floating constant when possible.
    fn constant_f64(&self, sig: SigId) -> Option<f64> {
        match match_sig(self.arena, sig) {
            SigMatch::Int(v) => Some(v as f64),
            SigMatch::Real(v) => Some(v),
            _ => None,
        }
    }

    /// Lowers one binary signal operator to FIR binop.
    fn lower_binop(&mut self, op: BinOp, lhs: SigId, rhs: SigId) -> Result<FirId, SignalFirError> {
        let lhs = self.lower_signal(lhs)?;
        let rhs = self.lower_signal(rhs)?;
        let real_ty = self.real_ty();
        let (fir_op, typ) = map_binop(op, real_ty).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedBinOp,
                format!("unsupported SIGBINOP operator `{}` in Step 2A", op.name()),
            )
        })?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.binop(fir_op, lhs, rhs, typ))
    }

    /// Lowers one unary math intrinsic call.
    fn lower_math1(&mut self, op: FirMathOp, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        self.used_math_ops.insert(op);
        // Math calls operate on and return the internal real type.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.math_call(op, &[value], real_ty))
    }

    /// Lowers one binary math intrinsic call.
    fn lower_math2(
        &mut self,
        op: FirMathOp,
        lhs: SigId,
        rhs: SigId,
    ) -> Result<FirId, SignalFirError> {
        let lhs = self.lower_signal(lhs)?;
        let rhs = self.lower_signal(rhs)?;
        self.used_math_ops.insert(op);
        // Math calls operate on and return the internal real type.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.math_call(op, &[lhs, rhs], real_ty))
    }

    /// Lowers one numeric cast.
    fn lower_cast(&mut self, typ: FirType, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.cast(typ, value))
    }

    /// Lowers one bitcast operation.
    fn lower_bitcast(&mut self, typ: FirType, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.bitcast(typ, value))
    }

    /// Lowers `select2` with explicit result-type selection.
    fn lower_select2(
        &mut self,
        cond: SigId,
        then_value: SigId,
        else_value: SigId,
    ) -> Result<FirId, SignalFirError> {
        let cond = self.lower_signal(cond)?;
        let then_value = self.lower_signal(then_value)?;
        let else_value = self.lower_signal(else_value)?;
        // select2 result is internal-precision.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.select2(cond, then_value, else_value, real_ty))
    }

    /// Lowers recursion projection nodes by decoding recursive groups in de
    /// Bruijn or symbolic form and recursively lowering the resolved group body.
    fn lower_proj(
        &mut self,
        node: SigId,
        index: i32,
        group: SigId,
    ) -> Result<FirId, SignalFirError> {
        if index != 0 {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGPROJ index {index} unsupported in Step 2C.2 (only 0)"),
            ));
        }

        if let Some(depth) = self.decode_debruijn_ref(group) {
            if depth == 0 || depth > self.recursion_stack.len() {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("invalid DEBRUIJNREF depth {depth}"),
                ));
            }
            let name = self.recursion_stack[self.recursion_stack.len() - depth].clone();
            // Recursion state slots hold internal-precision samples.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var(name, AccessType::Struct, real_ty));
        }

        if let Some(var) = match_sym_ref(self.arena, group) {
            let depth = self
                .recursion_vars
                .iter()
                .rposition(|bound| *bound == var)
                .map(|slot| self.recursion_vars.len() - slot)
                .ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        format!("unbound symbolic recursion variable {}", var.as_u32()),
                    )
                })?;
            let name = self.recursion_stack[self.recursion_stack.len() - depth].clone();
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var(name, AccessType::Struct, real_ty));
        }

        let body = if let Some(body) = self.decode_debruijn_group(group) {
            body
        } else if let Some(body) = self.decode_symbolic_group(group) {
            body
        } else if let SigMatch::Rec(body) = match_sig(self.arena, group) {
            body
        } else {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGPROJ group must be DEBRUIJN/DEBRUIJNREF/SYMREC/SYMREF/SIGREC in Step 2C.2 (expr={})",
                    dump_sig_readable(self.arena, node)
                ),
            ));
        };

        let init = self.float_const(0.0);
        let name = self.ensure_state_slot(node, init);
        let out = {
            // Recursion projection reads from an internal-precision state slot.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            b.load_var(name.clone(), AccessType::Struct, real_ty)
        };
        if self.scheduled_state_updates.insert(node) {
            if let Some((var, _)) = match_sym_rec(self.arena, group) {
                self.recursion_vars.push(var);
            }
            self.recursion_stack.push(name.clone());
            let rhs = self.lower_signal(body)?;
            self.recursion_stack.pop();
            if match_sym_rec(self.arena, group).is_some() {
                self.recursion_vars.pop();
            }
            let mut b = FirBuilder::new(&mut self.store);
            self.compute_updates
                .push(b.store_var(name, AccessType::Struct, rhs));
        }
        Ok(out)
    }

    /// Decodes one `SIGREC`/legacy recursion wrapper to its body signal.
    fn decode_debruijn_group(&self, group: SigId) -> Option<SigId> {
        let node = self.arena.node(group)?;
        let NodeKind::Tag(tag_id) = node.kind else {
            return None;
        };
        if self.arena.tag_name(tag_id)? != "DEBRUIJN" {
            return None;
        }
        let [list] = node.children.as_slice() else {
            return None;
        };
        self.arena.hd(*list)
    }

    /// Decodes one `SYMREC(var, body_list)` group to its first payload signal.
    ///
    /// `de_bruijn_to_sym` preserves the list-shaped recursive payload used by
    /// propagated signal groups, so the FIR lowerer must keep decoding the first
    /// element instead of assuming one direct body child.
    fn decode_symbolic_group(&self, group: SigId) -> Option<SigId> {
        let (_, body_list) = match_sym_rec(self.arena, group)?;
        self.arena.hd(body_list)
    }

    /// Decodes one `DEBRUIJNREF(level)` index from a recursion group payload.
    fn decode_debruijn_ref(&self, group: SigId) -> Option<usize> {
        let node = self.arena.node(group)?;
        let NodeKind::Tag(tag_id) = node.kind else {
            return None;
        };
        if self.arena.tag_name(tag_id)? != "DEBRUIJNREF" {
            return None;
        }
        let [depth] = node.children.as_slice() else {
            return None;
        };
        usize::try_from(tree_to_int(self.arena, *depth)?).ok()
    }
}

/// Maps signal-level operators to FIR operators with result typing policy.
///
/// `real_ty` is the internal DSP computation type (e.g. `Float32` / `Float64`).
/// It is used for arithmetic operators whose result is a real-valued sample.
/// Comparison operators always produce `Bool`; bitwise operators always produce
/// `Int32` — both are independent of `real_ty`.
fn map_binop(op: BinOp, real_ty: FirType) -> Option<(FirBinOp, FirType)> {
    match op {
        // Arithmetic operators: result is the internal real type.
        BinOp::Add => Some((FirBinOp::Add, real_ty)),
        BinOp::Sub => Some((FirBinOp::Sub, real_ty)),
        BinOp::Mul => Some((FirBinOp::Mul, real_ty)),
        BinOp::Div => Some((FirBinOp::Div, real_ty)),
        BinOp::Rem => Some((FirBinOp::Rem, real_ty)),
        // Comparison operators: result is boolean — independent of real_ty.
        BinOp::Gt => Some((FirBinOp::Gt, FirType::Bool)),
        BinOp::Lt => Some((FirBinOp::Lt, FirType::Bool)),
        BinOp::Ge => Some((FirBinOp::Ge, FirType::Bool)),
        BinOp::Le => Some((FirBinOp::Le, FirType::Bool)),
        BinOp::Eq => Some((FirBinOp::Eq, FirType::Bool)),
        BinOp::Ne => Some((FirBinOp::Ne, FirType::Bool)),
        // Bitwise operators: result is Int32 — independent of real_ty.
        BinOp::And => Some((FirBinOp::And, FirType::Int32)),
        BinOp::Or => Some((FirBinOp::Or, FirType::Int32)),
        BinOp::Xor => Some((FirBinOp::Xor, FirType::Int32)),
        BinOp::Lsh | BinOp::ARsh | BinOp::LRsh => None,
    }
}
