//! FIR module skeleton emission for the signal->FIR fast-lane.
//!
//! Step 1A emits a contract-only FIR module:
//! - module root exists and is structurally valid,
//! - a placeholder `compute` function is present,
//! - no signal semantics are emitted yet (planned for Step 2+).

use fir::{FirBuilder, FirId, FirStore, FirType};

use super::SignalFirOutput;
use super::planner::SignalFirPlan;

/// Emits a minimal FIR module from the validated planning snapshot.
#[must_use]
pub fn build_module(plan: &SignalFirPlan, module_name: &str) -> SignalFirOutput {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let mut body = Vec::new();
    body.push(b.label("signal_fir_fastlane_step1a: contract-only skeleton"));
    body.push(b.label(format!(
        "io: inputs={} outputs={}",
        plan.num_inputs, plan.num_outputs
    )));
    body.push(b.label(format!("signals: {}", plan.signal_count)));

    let compute_body = b.block(&body);
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: Vec::new(),
            ret: Box::new(FirType::Void),
        },
        &[],
        compute_body,
        false,
    );

    let declarations = b.block(&[compute]);
    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);
    let module: FirId = b.module(module_name, dsp_struct, globals, declarations);

    SignalFirOutput { store, module }
}
