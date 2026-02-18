//! FIR module emission for the signal->FIR fast-lane.
//!
//! Step 2A/2B lowers an executable bootstrap signal slice:
//! - `SIGINPUT`, integer/real constants,
//! - `SIGBINOP` (arithmetic/comparison/bitwise subset),
//! - `SIGPOW`/`SIGMIN`/`SIGMAX`,
//! - core unary math nodes (`sin/cos/tan/exp/log/log10/sqrt/abs`),
//! - `SIGDELAY1`/`SIGDELAY`/`SIGPREFIX`,
//! - `SIGSELECT2`, `SIGINTCAST`/`SIGFLOATCAST`/`SIGBITCAST`,
//! - `SIGPROJ`/`SIGREC` (real lowering for canonical `DEBRUIJN`/`DEBRUIJNREF` recursion).
//! - `SIGOUTPUT` passthrough nodes.
//!
//! Other signal families still return typed `FRS-SFIR-*` errors.

use std::collections::{HashMap, HashSet};

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};
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
    let mut statements = Vec::new();

    {
        let mut b = FirBuilder::new(&mut lower.store);
        statements.push(b.label("signal_fir_fastlane_step2a: executable base slice"));
        statements.push(b.label(format!(
            "io: inputs={} outputs={}",
            plan.num_inputs, plan.num_outputs
        )));
        statements.push(b.label(format!("signals: {}", plan.signal_count)));
    }

    for sig in signals {
        let value = lower.lower_signal(*sig)?;
        let mut b = FirBuilder::new(&mut lower.store);
        statements.push(b.drop_(value));
    }
    statements.extend(lower.compute_updates.iter().copied());

    let compute_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&statements)
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
        b.block(&[compute])
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
    compute_updates: Vec<FirId>,
    state_name_by_node: HashMap<SigId, String>,
    scheduled_state_updates: HashSet<SigId>,
    recursion_stack: Vec<String>,
}

impl<'a> SignalToFirLower<'a> {
    fn new(arena: &'a TreeArena, num_inputs: usize) -> Self {
        Self {
            arena,
            num_inputs,
            store: FirStore::new(),
            cache: HashMap::new(),
            struct_declarations: Vec::new(),
            compute_updates: Vec::new(),
            state_name_by_node: HashMap::new(),
            scheduled_state_updates: HashSet::new(),
            recursion_stack: Vec::new(),
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
            SigMatch::Tan(value) => self.lower_fun1("std::tan", value)?,
            SigMatch::Exp(value) => self.lower_fun1("std::exp", value)?,
            SigMatch::Log(value) => self.lower_fun1("std::log", value)?,
            SigMatch::Log10(value) => self.lower_fun1("std::log10", value)?,
            SigMatch::Sqrt(value) => self.lower_fun1("std::sqrt", value)?,
            SigMatch::Abs(value) => self.lower_fun1("std::fabs", value)?,
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
        let name = format!("frs_state_n{}", node.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(
            name.clone(),
            FirType::FaustFloat,
            AccessType::Struct,
            Some(init),
        );
        self.struct_declarations.push(dec);
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
