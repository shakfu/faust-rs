//! Table-signal helpers shared by the lowerer and the boundary checks.

use crate::signal_fir::vector::analysis::wrtbl_is_readonly;
use crate::signal_prepare::VerifiedPreparedSignals;
use fir::FirType;
use signals::{SigId, SigMatch, match_sig};
use std::collections::BTreeMap;
/// Canonical DSP-struct field name for one mutable table.
///
/// Shared by the lowerer and the final-module verifier so the emitted
/// declaration, the compute stores, and the attribution check cannot drift
/// onto different names.
pub(in crate::signal_fir::vector) fn mutable_table_name(
    signal_id: u64,
    elem_type: &FirType,
) -> String {
    let prefix = if *elem_type == FirType::Int32 {
        "iVecMutTbl"
    } else {
        "fVecMutTbl"
    };
    format!("{prefix}{signal_id}")
}
pub(super) fn mutable_table_signal(
    prepared: &VerifiedPreparedSignals,
    ids: &BTreeMap<u64, SigId>,
    signal_id: u64,
) -> bool {
    ids.get(&signal_id).is_some_and(|&sig| {
        matches!(
            match_sig(prepared.arena(), sig),
            SigMatch::WrTbl(_, _, write_index, write_value)
                if !wrtbl_is_readonly(prepared.arena(), write_index, write_value)
        )
    })
}
pub(super) fn readonly_table_signal(
    prepared: &VerifiedPreparedSignals,
    ids: &BTreeMap<u64, SigId>,
    signal_id: u64,
) -> bool {
    ids.get(&signal_id).is_some_and(|&sig| {
        matches!(
            match_sig(prepared.arena(), sig),
            SigMatch::WrTbl(_, _, write_index, write_value)
                if prepared.arena().is_nil(write_index)
                    && prepared.arena().is_nil(write_value)
        )
    })
}
