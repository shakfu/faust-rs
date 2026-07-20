//! Output materialization for the final vector module.

use super::build::VectorModuleFailure;
use crate::signal_fir::VectorFallbackReason;
use crate::signal_fir::vector::assemble::{VectorClockOutputStore, VectorLoopFirInput};
use crate::signal_fir::vector::route::{VectorRegion, VerifiedRoutedFir};
use fir::{AccessType, FirBuilder, FirId, FirStore, FirType};
use signals::SigId;
use std::collections::BTreeMap;
pub(super) struct OutputMaterialization<'a> {
    pub(super) routed: &'a VerifiedRoutedFir,
    pub(super) loop_inputs: &'a mut [VectorLoopFirInput],
    pub(super) control_statements: &'a mut Vec<FirId>,
    pub(super) control_output_stores: &'a mut Vec<FirId>,
    pub(super) clock_output_stores: &'a mut Vec<VectorClockOutputStore>,
    pub(super) clock_plan: &'a crate::signal_fir::vector::clock_ad::VerifiedVectorClockAdPlan,
    pub(super) store: &'a mut FirStore,
}
pub(super) fn materialize_outputs(
    outputs: &[SigId],
    num_outputs: usize,
    context: &mut OutputMaterialization<'_>,
) -> Result<Vec<FirId>, VectorModuleFailure> {
    if outputs.len() != num_outputs {
        return Err(VectorModuleFailure::new(
            VectorFallbackReason::OutputAssembly,
            "prepared output count does not match the module contract",
        ));
    }
    let loop_index = context
        .loop_inputs
        .iter()
        .enumerate()
        .map(|(index, body)| (body.loop_id, index))
        .collect::<BTreeMap<_, _>>();
    let mut stores = Vec::with_capacity(outputs.len());
    for (channel, output) in outputs.iter().enumerate() {
        let signal_id = u64::from(output.as_u32());
        let signal = context
            .routed
            .plan()
            .signals
            .iter()
            .find(|signal| signal.signal_id == signal_id)
            .ok_or_else(|| {
                VectorModuleFailure::new(
                    VectorFallbackReason::OutputAssembly,
                    format!("output signal {signal_id} is absent from the vector plan"),
                )
            })?;
        let definition_region = match signal.placement {
            crate::signal_fir::vector::verify::Placement::Owned(loop_id) => {
                VectorRegion::Loop(loop_id)
            }
            crate::signal_fir::vector::verify::Placement::Control => VectorRegion::Control,
            crate::signal_fir::vector::verify::Placement::Inline => {
                return Err(VectorModuleFailure::new(
                    VectorFallbackReason::OutputAssembly,
                    format!("output signal {signal_id} remains inline at final assembly"),
                ));
            }
        };
        let value = context
            .routed
            .trace()
            .definitions()
            .iter()
            .find(|definition| {
                definition.signal_id == signal_id && definition.region == definition_region
            })
            .map(|definition| definition.value)
            .ok_or_else(|| {
                VectorModuleFailure::new(
                    VectorFallbackReason::OutputAssembly,
                    format!("output signal {signal_id} has no routed FIR definition"),
                )
            })?;
        let value_is_faust_float = context.store.value_type(value) == Some(FirType::FaustFloat);
        let mut builder = FirBuilder::new(context.store);
        let channel_i32 = i32::try_from(channel).map_err(|_| {
            VectorModuleFailure::new(
                VectorFallbackReason::OutputAssembly,
                "output channel index exceeds FIR i32",
            )
        })?;
        let channel_value = builder.int32(channel_i32);
        let pointer_type = FirType::Ptr(Box::new(FirType::FaustFloat));
        let pointer = builder.load_table(
            "outputs",
            AccessType::FunArgs,
            channel_value,
            pointer_type.clone(),
        );
        context.control_statements.push(builder.declare_var(
            format!("output{channel}"),
            pointer_type,
            AccessType::Stack,
            Some(pointer),
        ));
        let value = if value_is_faust_float {
            value
        } else {
            builder.cast(FirType::FaustFloat, value)
        };
        let sample = builder.load_var("i0", AccessType::Loop, FirType::Int32);
        let output_store =
            builder.store_table(format!("output{channel}"), AccessType::Stack, sample, value);
        match definition_region {
            VectorRegion::Loop(loop_id) => {
                let is_clock_owned = context
                    .clock_plan
                    .plan()
                    .clock_islands
                    .iter()
                    .any(|island| island.nested_loop_ids.contains(&loop_id));
                if is_clock_owned {
                    context.clock_output_stores.push(VectorClockOutputStore {
                        owner_loop_id: loop_id,
                        statement: output_store,
                    });
                } else {
                    let region_index = *loop_index.get(&loop_id).ok_or_else(|| {
                        VectorModuleFailure::new(
                            VectorFallbackReason::OutputAssembly,
                            format!("output loop {loop_id} has no final region body"),
                        )
                    })?;
                    context.loop_inputs[region_index]
                        .statements
                        .push(output_store);
                }
            }
            VectorRegion::Control => context.control_output_stores.push(output_store),
        }
        stores.push(output_store);
    }
    Ok(stores)
}
