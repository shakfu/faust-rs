//! Independent checker and the shared terminal verify path.
//!
//! `verify_vector_clock_ad_plan_after_vector_plan` is called by BOTH the
//! producer's terminal verification (`build_vector_clock_ad_plan`) and the
//! standalone checker (`verify_vector_clock_ad_plan`), so every admission
//! guard it hosts — including `reject_unadopted_stateful_reads` — remains
//! on both paths after the R6 split (plan §4.8).

use super::build::{derive_clock_islands, derive_reverse_fallbacks, derive_transport_policies};
use super::model::*;
use crate::clk_env::annotate;
use crate::signal_fir::vector::analysis::DepKind;
use crate::signal_fir::vector::decoration_verify::VerifiedDecorationCertificate;
use crate::signal_fir::vector::verify::{VectorPlan, verify_vector_plan};
use crate::signal_prepare::VerifiedPreparedSignals;
use propagate::ClockDomainTable;
use signals::{SigId, SigMatch, match_sig};
use std::collections::{BTreeMap, BTreeSet};
use tlib::{match_sym_rec, match_sym_ref};

/// Recomputes all island, transport, and reverse-fallback obligations.
pub fn verify_vector_clock_ad_plan(
    prepared: &VerifiedPreparedSignals,
    domains: &ClockDomainTable,
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VectorPlan,
    clock_ad_plan: &VectorClockAdPlan,
) -> Result<(), VectorClockAdError> {
    verify_vector_plan(vector_plan)?;
    verify_vector_clock_ad_plan_after_vector_plan(
        prepared,
        domains,
        decorations,
        vector_plan,
        clock_ad_plan,
    )
}
pub(super) fn verify_vector_clock_ad_plan_after_vector_plan(
    prepared: &VerifiedPreparedSignals,
    domains: &ClockDomainTable,
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VectorPlan,
    clock_ad_plan: &VectorClockAdPlan,
) -> Result<(), VectorClockAdError> {
    if clock_ad_plan.schema_version != VECTOR_CLOCK_AD_PLAN_VERSION {
        return Err(VectorClockAdError::UnsupportedSchema {
            found: clock_ad_plan.schema_version,
        });
    }
    if clock_ad_plan.vec_size != vector_plan.vec_size {
        return Err(VectorClockAdError::VecSizeMismatch {
            declared: clock_ad_plan.vec_size,
            actual: vector_plan.vec_size,
        });
    }
    verify_source_alignment(prepared, domains, decorations, vector_plan)?;
    let expected_islands = derive_clock_islands(prepared, domains, decorations, vector_plan)?;
    if clock_ad_plan.clock_islands != expected_islands {
        return Err(VectorClockAdError::IslandCoverageMismatch);
    }
    reject_unadopted_stateful_reads(prepared, decorations, vector_plan, &expected_islands)?;
    let expected_transports = derive_transport_policies(prepared, vector_plan)?;
    if clock_ad_plan.transports != expected_transports {
        return Err(VectorClockAdError::TransportCoverageMismatch);
    }
    if clock_ad_plan.forward_ad != ForwardAdPolicy::ExpandedSignalGraph {
        return Err(VectorClockAdError::ReverseAdCoverageMismatch);
    }
    let expected_reverse = derive_reverse_fallbacks(prepared, decorations, vector_plan)?;
    if clock_ad_plan.reverse_ad_fallbacks != expected_reverse {
        return Err(VectorClockAdError::ReverseAdCoverageMismatch);
    }
    Ok(())
}
/// Rejects fire-time reads of audio-rate stateful values.
///
/// Clock inference is rate-polymorphic: a recursion or delay whose inputs are
/// all constants infers the bottom environment, so the plan hoists it into an
/// outer-rate loop even when it is written under a clock wrapper (e.g.
/// `upsampling(1 : (+ ~ *(0.5)))`). Its state then advances once per outer
/// sample instead of once per fire — a miscompile the reference semantics
/// reject. Until such groups are adopted into their wrapper's domain, any
/// value dependency from an in-domain consumer to an audio-rate stateful
/// producer fails closed. The one legal crossing is the `ZeroPad` value
/// operand: propagation inserts it exactly where an outer-rate producer is
/// zero-stuffed into an upsampled domain.
pub(super) fn reject_unadopted_stateful_reads(
    prepared: &VerifiedPreparedSignals,
    decorations: &VerifiedDecorationCertificate,
    plan: &VectorPlan,
    islands: &[ClockIsland],
) -> Result<(), VectorClockAdError> {
    let ids = crate::signal_fir::vector::common::ids::prepared_signal_ids(prepared);
    let signals = plan
        .signals
        .iter()
        .map(|signal| (signal.signal_id, signal))
        .collect::<BTreeMap<_, _>>();
    let domain_by_clock = islands
        .iter()
        .map(|island| (island.domain_id + 1, island.domain_id))
        .collect::<BTreeMap<_, _>>();
    for dependency in &decorations.certificate().dependencies {
        if !matches!(
            dependency.kind,
            DepKind::Immediate | DepKind::Delayed { .. }
        ) {
            continue;
        }
        let Some(consumer) = signals.get(&u64::from(dependency.from)) else {
            continue;
        };
        let Some(producer) = signals.get(&u64::from(dependency.to)) else {
            continue;
        };
        if producer.clock_id != 0 {
            continue;
        }
        let Some(&domain_id) = domain_by_clock.get(&consumer.clock_id) else {
            continue;
        };
        let Some(&producer_sig) = ids.get(&dependency.to) else {
            continue;
        };
        if !is_stateful_producer(prepared, producer_sig) {
            continue;
        }
        let consumer_sig = ids.get(&dependency.from).copied();
        if consumer_sig
            .is_some_and(|sig| matches!(match_sig(prepared.arena(), sig), SigMatch::ZeroPad(_, _)))
        {
            continue;
        }
        return Err(VectorClockAdError::UnadoptedStatefulRead {
            domain_id,
            consumer: u64::from(dependency.from),
            producer: u64::from(dependency.to),
        });
    }
    Ok(())
}
/// Whether the signal carries per-sample state whose advance rate is
/// observable: symbolic recursion (group, reference, or projection) and
/// explicit delays.
pub(super) fn is_stateful_producer(prepared: &VerifiedPreparedSignals, sig: SigId) -> bool {
    let arena = prepared.arena();
    if match_sym_rec(arena, sig).is_some() || match_sym_ref(arena, sig).is_some() {
        return true;
    }
    match match_sig(arena, sig) {
        SigMatch::Delay1(_) | SigMatch::Delay(_, _) | SigMatch::Prefix(_, _) => true,
        SigMatch::Proj(_, group) => {
            match_sym_rec(arena, group).is_some() || match_sym_ref(arena, group).is_some()
        }
        _ => false,
    }
}
pub(super) fn verify_source_alignment(
    prepared: &VerifiedPreparedSignals,
    domains: &ClockDomainTable,
    decorations: &VerifiedDecorationCertificate,
    plan: &VectorPlan,
) -> Result<(), VectorClockAdError> {
    let records = &decorations.certificate().records;
    if records.len() != plan.signals.len() {
        return Err(VectorClockAdError::SignalCoverageMismatch {
            signal_id: u64::MAX,
        });
    }
    let clocks = annotate(prepared.arena(), domains, prepared.outputs())?;
    let ids = crate::signal_fir::vector::common::ids::prepared_signal_ids(prepared);
    for (record, signal) in records.iter().zip(&plan.signals) {
        let signal_id = u64::from(record.signal_id);
        if signal.signal_id != signal_id {
            return Err(VectorClockAdError::SignalCoverageMismatch { signal_id });
        }
        let sig = ids
            .get(&record.signal_id)
            .copied()
            .ok_or(VectorClockAdError::SignalCoverageMismatch { signal_id })?;
        let inferred = clocks
            .env(sig)
            .ok_or(VectorClockAdError::ClockFactMismatch { signal_id })?
            .map(|domain| domain.as_u32());
        if record.clock_domain != inferred
            || signal.clock_id != inferred.map_or(0, |domain| u64::from(domain) + 1)
        {
            return Err(VectorClockAdError::ClockFactMismatch { signal_id });
        }
    }
    verify_domain_tree(domains)
}
pub(super) fn verify_domain_tree(domains: &ClockDomainTable) -> Result<(), VectorClockAdError> {
    for (domain_id, domain) in domains.iter() {
        let raw = u64::from(domain_id.as_u32());
        if let Some(parent) = domain.parent
            && domains.get(parent).is_none()
        {
            return Err(VectorClockAdError::DomainParentUnknown {
                domain_id: raw,
                parent_id: u64::from(parent.as_u32()),
            });
        }
        let mut seen = BTreeSet::new();
        let mut cursor = Some(domain_id);
        while let Some(current) = cursor {
            if !seen.insert(current.as_u32()) {
                return Err(VectorClockAdError::DomainCycle { domain_id: raw });
            }
            cursor = domains.get(current).and_then(|entry| entry.parent);
        }
    }
    Ok(())
}
