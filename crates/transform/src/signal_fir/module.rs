//! FIR module emission for the signal->FIR fast-lane.
//!
//! Step 2A lowers a first executable signal slice:
//! - `SIGINPUT`, integer/real constants,
//! - `SIGBINOP` (arithmetic/comparison/bitwise subset),
//! - `SIGOUTPUT` passthrough nodes.
//!
//! Other signal families still return typed `FRS-SFIR-*` errors.

use std::collections::HashMap;

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};
use signals::{BinOp, SigId, SigMatch, dump_sig_readable, match_sig};
use tlib::TreeArena;

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
        b.block(&[])
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
}

impl<'a> SignalToFirLower<'a> {
    fn new(arena: &'a TreeArena, num_inputs: usize) -> Self {
        Self {
            arena,
            num_inputs,
            store: FirStore::new(),
            cache: HashMap::new(),
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
            SigMatch::BinOp(op, lhs, rhs) => self.lower_binop(op, lhs, rhs)?,
            other => {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "unsupported signal node in Step 2A: {other:?} (expr={})",
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
