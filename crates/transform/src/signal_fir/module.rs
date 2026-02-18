//! FIR module emission for the signal->FIR fast-lane.
//!
//! Step 2A..2G lowers an executable fast-lane slice:
//! - `SIGINPUT`, integer/real constants,
//! - `SIGBINOP` (arithmetic/comparison/bitwise subset),
//! - `SIGPOW`/`SIGMIN`/`SIGMAX`,
//! - core unary math nodes (`sin/cos/tan/exp/log/log10/sqrt/abs`),
//! - `SIGDELAY1`/`SIGDELAY`/`SIGPREFIX`,
//! - `SIGSELECT2`, `SIGINTCAST`/`SIGFLOATCAST`/`SIGBITCAST`,
//! - `SIGPROJ`/`SIGREC` (real lowering for canonical `DEBRUIJN`/`DEBRUIJNREF` recursion).
//! - `SIGWAVEFORM`/`SIGRDTBL`/`SIGWRTBL` for direct waveform tables.
//! - `SIGOUTPUT` passthrough nodes.
//! - sectioned FIR module assembly (`metadata`, `instanceConstants`,
//!   `instanceResetUserInterface`, `instanceClear`, `buildUserInterface`, `compute`).
//!
//! Other signal families still return typed `FRS-SFIR-*` errors.

use std::collections::{HashMap, HashSet};

use fir::{
    AccessType, BargraphType, ButtonType, FirBinOp, FirBuilder, FirId, FirStore, FirType,
    SliderRange, SliderType,
};
use signals::{BinOp, SigId, SigMatch, dump_sig_readable, match_sig};
use tlib::{NodeKind, TreeArena};

use super::SignalFirOutput;
use super::error::{SignalFirError, SignalFirErrorCode};
use super::planner::SignalFirPlan;

/// Emits a FIR module from validated planning data and propagated signals.
pub fn build_module(
    plan: &SignalFirPlan,
    module_name: &str,
    arena: &TreeArena,
    signals: &[SigId],
) -> Result<SignalFirOutput, SignalFirError> {
    let mut lower = SignalToFirLower::new(arena, plan.num_inputs);

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

    for sig in signals {
        let value = lower.lower_signal(*sig)?;
        let mut b = FirBuilder::new(&mut lower.store);
        lower.sample_statements.push(b.drop_(value));
    }
    lower
        .sample_statements
        .extend(lower.compute_updates.iter().copied());

    let metadata_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&[])
    };
    let metadata = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "metadata",
            FirType::Fun {
                args: Vec::new(),
                ret: Box::new(FirType::Void),
            },
            &[],
            metadata_body,
            false,
        )
    };

    let constants_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.constants_statements)
    };
    let instance_constants = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceConstants",
            FirType::Fun {
                args: Vec::new(),
                ret: Box::new(FirType::Void),
            },
            &[],
            constants_body,
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
                args: Vec::new(),
                ret: Box::new(FirType::Void),
            },
            &[],
            reset_body,
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
                args: Vec::new(),
                ret: Box::new(FirType::Void),
            },
            &[],
            clear_body,
            false,
        )
    };

    let ui_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.ui_statements)
    };
    let build_ui = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "buildUserInterface",
            FirType::Fun {
                args: Vec::new(),
                ret: Box::new(FirType::Void),
            },
            &[],
            ui_body,
            false,
        )
    };

    let compute_statements = {
        let mut all = Vec::new();
        all.extend(lower.control_statements.iter().copied());
        all.extend(lower.sample_statements.iter().copied());
        all
    };
    let compute_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&compute_statements)
    };
    let compute = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "compute",
            FirType::Fun {
                args: Vec::new(),
                ret: Box::new(FirType::Void),
            },
            &[],
            compute_body,
            false,
        )
    };

    let declarations = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&[
            metadata,
            instance_constants,
            instance_reset_ui,
            instance_clear,
            build_ui,
            compute,
        ])
    };
    let dsp_struct = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.struct_declarations)
    };
    let globals = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&[])
    };
    let module: FirId = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.module(module_name, dsp_struct, globals, declarations)
    };

    Ok(SignalFirOutput {
        store: lower.store,
        module,
    })
}

struct SignalToFirLower<'a> {
    arena: &'a TreeArena,
    num_inputs: usize,
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
    ui_controls: HashMap<SigId, String>,
    soundfiles: HashMap<SigId, String>,
    waveform_tables: HashMap<SigId, String>,
    waveform_table_len: HashMap<SigId, usize>,
    ui_statements: Vec<FirId>,
    named_struct_vars: HashSet<String>,
    reset_init_seen: HashSet<String>,
    clear_init_seen: HashSet<String>,
}

impl<'a> SignalToFirLower<'a> {
    fn new(arena: &'a TreeArena, num_inputs: usize) -> Self {
        Self {
            arena,
            num_inputs,
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
            ui_controls: HashMap::new(),
            soundfiles: HashMap::new(),
            waveform_tables: HashMap::new(),
            waveform_table_len: HashMap::new(),
            ui_statements: Vec::new(),
            named_struct_vars: HashSet::new(),
            reset_init_seen: HashSet::new(),
            clear_init_seen: HashSet::new(),
        }
    }

    fn lower_signal(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        if let Some(id) = self.cache.get(&sig).copied() {
            return Ok(id);
        }

        let lowered = match match_sig(self.arena, sig) {
            SigMatch::Int(value) => {
                let mut b = FirBuilder::new(&mut self.store);
                b.int64(value)
            }
            SigMatch::Real(value) => {
                let mut b = FirBuilder::new(&mut self.store);
                b.float64(value)
            }
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
            SigMatch::IntCast(value) => self.lower_cast(FirType::Int64, value)?,
            SigMatch::BitCast(value) => self.lower_bitcast(FirType::FaustFloat, value)?,
            SigMatch::FloatCast(value) => self.lower_cast(FirType::FaustFloat, value)?,
            SigMatch::Select2(cond, then_value, else_value) => {
                self.lower_select2(cond, then_value, else_value)?
            }
            SigMatch::Proj(index, group) => self.lower_proj(sig, index, group)?,
            SigMatch::Rec(body) => self.lower_signal(body)?,
            SigMatch::BinOp(op, lhs, rhs) => self.lower_binop(op, lhs, rhs)?,
            SigMatch::Pow(lhs, rhs) => self.lower_fun2("std::pow", lhs, rhs)?,
            SigMatch::Min(lhs, rhs) => self.lower_fun2("std::fmin", lhs, rhs)?,
            SigMatch::Max(lhs, rhs) => self.lower_fun2("std::fmax", lhs, rhs)?,
            SigMatch::Sin(value) => self.lower_fun1("std::sin", value)?,
            SigMatch::Cos(value) => self.lower_fun1("std::cos", value)?,
            SigMatch::Acos(value) => self.lower_fun1("std::acos", value)?,
            SigMatch::Asin(value) => self.lower_fun1("std::asin", value)?,
            SigMatch::Atan(value) => self.lower_fun1("std::atan", value)?,
            SigMatch::Atan2(lhs, rhs) => self.lower_fun2("std::atan2", lhs, rhs)?,
            SigMatch::Tan(value) => self.lower_fun1("std::tan", value)?,
            SigMatch::Exp(value) => self.lower_fun1("std::exp", value)?,
            SigMatch::Log(value) => self.lower_fun1("std::log", value)?,
            SigMatch::Log10(value) => self.lower_fun1("std::log10", value)?,
            SigMatch::Sqrt(value) => self.lower_fun1("std::sqrt", value)?,
            SigMatch::Abs(value) => self.lower_fun1("std::fabs", value)?,
            SigMatch::Fmod(lhs, rhs) => self.lower_fun2("std::fmod", lhs, rhs)?,
            SigMatch::Remainder(lhs, rhs) => self.lower_fun2("std::remainder", lhs, rhs)?,
            SigMatch::Floor(value) => self.lower_fun1("std::floor", value)?,
            SigMatch::Ceil(value) => self.lower_fun1("std::ceil", value)?,
            SigMatch::Rint(value) => self.lower_fun1("std::rint", value)?,
            SigMatch::Round(value) => self.lower_fun1("std::round", value)?,
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
                let mut b = FirBuilder::new(&mut self.store);
                b.select2(cond, lhs, zero, FirType::FaustFloat)
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

    fn lower_input(&mut self, index: i64) -> Result<FirId, SignalFirError> {
        if index < 0 {
            return Err(SignalFirError::new(
                SignalFirErrorCode::InputIndexOutOfRange,
                format!("input index must be >= 0, got {index}"),
            ));
        }
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

        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_var(
            format!("input{index}[i0]"),
            AccessType::FunArgs,
            FirType::FaustFloat,
        ))
    }

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

    fn lower_delay_state(
        &mut self,
        node: SigId,
        value: SigId,
        init: FirId,
    ) -> Result<FirId, SignalFirError> {
        let name = self.ensure_state_slot(node, init);
        let out = {
            let mut b = FirBuilder::new(&mut self.store);
            b.load_var(name.clone(), AccessType::Struct, FirType::FaustFloat)
        };
        if self.scheduled_state_updates.insert(node) {
            let next = self.lower_signal(value)?;
            let mut b = FirBuilder::new(&mut self.store);
            self.compute_updates
                .push(b.store_var(name, AccessType::Struct, next));
        }
        Ok(out)
    }

    fn ensure_state_slot(&mut self, node: SigId, init: FirId) -> String {
        if let Some(name) = self.state_name_by_node.get(&node) {
            return name.clone();
        }
        let name = format!("state_n{}", node.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(
            name.clone(),
            FirType::FaustFloat,
            AccessType::Struct,
            Some(init),
        );
        self.struct_declarations.push(dec);
        self.register_clear_init(name.clone(), init);
        self.state_name_by_node.insert(node, name.clone());
        name
    }

    fn float_const(&mut self, value: f64) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        b.float64(value)
    }

    fn initial_state_from_signal(&mut self, sig: SigId) -> FirId {
        match match_sig(self.arena, sig) {
            SigMatch::Int(v) => {
                let mut b = FirBuilder::new(&mut self.store);
                b.int64(v)
            }
            SigMatch::Real(v) => self.float_const(v),
            _ => self.float_const(0.0),
        }
    }

    fn lower_button(&mut self, node: SigId, label: SigId, typ: ButtonType) -> FirId {
        if let Some(var) = self.ui_controls.get(&node).cloned() {
            let mut b = FirBuilder::new(&mut self.store);
            return b.load_var(var, AccessType::Struct, FirType::FaustFloat);
        }
        let var = format!("fUiCtl{}", node.as_u32());
        let init = self.float_const(0.0);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        let label = self.label_text(label);
        let mut b = FirBuilder::new(&mut self.store);
        self.ui_statements
            .push(b.add_button(typ, label, var.clone()));
        self.ui_controls.insert(node, var.clone());
        b.load_var(var, AccessType::Struct, FirType::FaustFloat)
    }

    fn lower_slider(
        &mut self,
        node: SigId,
        params: [SigId; 5],
        typ: SliderType,
    ) -> Result<FirId, SignalFirError> {
        let [label, init, min, max, step] = params;
        if let Some(var) = self.ui_controls.get(&node).cloned() {
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var(var, AccessType::Struct, FirType::FaustFloat));
        }
        let var = format!("fUiCtl{}", node.as_u32());
        let init_v = self.constant_f64(init).unwrap_or(0.0);
        let min_v = self.constant_f64(min).unwrap_or(0.0);
        let max_v = self.constant_f64(max).unwrap_or(1.0);
        let step_v = self.constant_f64(step).unwrap_or(0.01);
        let init_id = self.float_const(init_v);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init_id));
        let label = self.label_text(label);
        let range = SliderRange {
            init: init_v,
            lo: min_v,
            hi: max_v,
            step: step_v,
        };
        let mut b = FirBuilder::new(&mut self.store);
        self.ui_statements
            .push(b.add_slider(typ, label, var.clone(), range));
        self.ui_controls.insert(node, var.clone());
        Ok(b.load_var(var, AccessType::Struct, FirType::FaustFloat))
    }

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
            let var = format!("fUiMeter{}", node.as_u32());
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
        self.lower_signal(value)
    }

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

    fn lower_waveform(&mut self, node: SigId, values: &[SigId]) -> Result<FirId, SignalFirError> {
        let table_name = self.ensure_waveform_table(node, values)?;
        let index = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(0)
        };
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(table_name, AccessType::Struct, index, FirType::FaustFloat))
    }

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
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(table_name, AccessType::Struct, index, FirType::FaustFloat))
    }

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
        let name = format!("table_n{}", sig.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(
            name.clone(),
            AccessType::Struct,
            FirType::FaustFloat,
            &lowered_values,
        );
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

    fn ensure_wrtbl_table(
        &mut self,
        sig: SigId,
        size_sig: SigId,
        generator_sig: SigId,
    ) -> Result<(String, usize), SignalFirError> {
        let size = self.table_size_from_sig(size_sig)?;
        let generated = self.expand_generator_values(generator_sig, size)?;
        let name = format!("table_n{}", sig.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(
            name.clone(),
            AccessType::Struct,
            FirType::FaustFloat,
            &generated,
        );
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

    fn ensure_named_struct_var(&mut self, name: &str, typ: FirType, init: Option<FirId>) {
        if self.named_struct_vars.contains(name) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(name.to_owned(), typ, AccessType::Struct, init);
        self.struct_declarations.push(dec);
        self.named_struct_vars.insert(name.to_owned());
        if let Some(init) = init {
            self.register_reset_init(name.to_owned(), init);
        }
    }

    fn register_reset_init(&mut self, name: String, init: FirId) {
        if !self.reset_init_seen.insert(name.clone()) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.reset_statements
            .push(b.store_var(name, AccessType::Struct, init));
    }

    fn register_clear_init(&mut self, name: String, init: FirId) {
        if !self.clear_init_seen.insert(name.clone()) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.clear_statements
            .push(b.store_var(name, AccessType::Struct, init));
    }

    fn unsupported_node<T>(&self, sig: SigId, detail: &str) -> Result<T, SignalFirError> {
        Err(SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("{detail} (expr={})", dump_sig_readable(self.arena, sig)),
        ))
    }

    fn label_text(&self, label: SigId) -> String {
        match self.arena.kind(label) {
            Some(NodeKind::Symbol(s)) => s.to_string(),
            Some(NodeKind::StringLiteral(s)) => s.to_string(),
            Some(NodeKind::Int(v)) => v.to_string(),
            Some(NodeKind::FloatBits(bits)) => f64::from_bits(*bits).to_string(),
            _ => "ui".to_owned(),
        }
    }

    fn constant_f64(&self, sig: SigId) -> Option<f64> {
        match match_sig(self.arena, sig) {
            SigMatch::Int(v) => Some(v as f64),
            SigMatch::Real(v) => Some(v),
            _ => None,
        }
    }

    fn lower_binop(&mut self, op: BinOp, lhs: SigId, rhs: SigId) -> Result<FirId, SignalFirError> {
        let lhs = self.lower_signal(lhs)?;
        let rhs = self.lower_signal(rhs)?;
        let (fir_op, typ) = map_binop(op).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedBinOp,
                format!("unsupported SIGBINOP operator `{}` in Step 2A", op.name()),
            )
        })?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.binop(fir_op, lhs, rhs, typ))
    }

    fn lower_fun1(&mut self, name: &str, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.fun_call(name, &[value], FirType::FaustFloat))
    }

    fn lower_fun2(&mut self, name: &str, lhs: SigId, rhs: SigId) -> Result<FirId, SignalFirError> {
        let lhs = self.lower_signal(lhs)?;
        let rhs = self.lower_signal(rhs)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.fun_call(name, &[lhs, rhs], FirType::FaustFloat))
    }

    fn lower_cast(&mut self, typ: FirType, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.cast(typ, value))
    }

    fn lower_bitcast(&mut self, typ: FirType, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.bitcast(typ, value))
    }

    fn lower_select2(
        &mut self,
        cond: SigId,
        then_value: SigId,
        else_value: SigId,
    ) -> Result<FirId, SignalFirError> {
        let cond = self.lower_signal(cond)?;
        let then_value = self.lower_signal(then_value)?;
        let else_value = self.lower_signal(else_value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.select2(cond, then_value, else_value, FirType::FaustFloat))
    }

    fn lower_proj(
        &mut self,
        node: SigId,
        index: i64,
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
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var(name, AccessType::Struct, FirType::FaustFloat));
        }

        let body = if let Some(body) = self.decode_debruijn_group(group) {
            body
        } else if let SigMatch::Rec(body) = match_sig(self.arena, group) {
            body
        } else {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGPROJ group must be DEBRUIJN/DEBRUIJNREF/SIGREC in Step 2C.2 (expr={})",
                    dump_sig_readable(self.arena, node)
                ),
            ));
        };

        let init = self.float_const(0.0);
        let name = self.ensure_state_slot(node, init);
        let out = {
            let mut b = FirBuilder::new(&mut self.store);
            b.load_var(name.clone(), AccessType::Struct, FirType::FaustFloat)
        };
        if self.scheduled_state_updates.insert(node) {
            self.recursion_stack.push(name.clone());
            let rhs = self.lower_signal(body)?;
            self.recursion_stack.pop();
            let mut b = FirBuilder::new(&mut self.store);
            self.compute_updates
                .push(b.store_var(name, AccessType::Struct, rhs));
        }
        Ok(out)
    }

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
        match self.arena.kind(*depth) {
            Some(NodeKind::Int(v)) => usize::try_from(*v).ok(),
            _ => None,
        }
    }
}

fn map_binop(op: BinOp) -> Option<(FirBinOp, FirType)> {
    match op {
        BinOp::Add => Some((FirBinOp::Add, FirType::FaustFloat)),
        BinOp::Sub => Some((FirBinOp::Sub, FirType::FaustFloat)),
        BinOp::Mul => Some((FirBinOp::Mul, FirType::FaustFloat)),
        BinOp::Div => Some((FirBinOp::Div, FirType::FaustFloat)),
        BinOp::Rem => Some((FirBinOp::Rem, FirType::FaustFloat)),
        BinOp::Gt => Some((FirBinOp::Gt, FirType::Bool)),
        BinOp::Lt => Some((FirBinOp::Lt, FirType::Bool)),
        BinOp::Ge => Some((FirBinOp::Ge, FirType::Bool)),
        BinOp::Le => Some((FirBinOp::Le, FirType::Bool)),
        BinOp::Eq => Some((FirBinOp::Eq, FirType::Bool)),
        BinOp::Ne => Some((FirBinOp::Ne, FirType::Bool)),
        BinOp::And => Some((FirBinOp::And, FirType::Int64)),
        BinOp::Or => Some((FirBinOp::Or, FirType::Int64)),
        BinOp::Xor => Some((FirBinOp::Xor, FirType::Int64)),
        BinOp::Lsh | BinOp::ARsh | BinOp::LRsh => None,
    }
}
