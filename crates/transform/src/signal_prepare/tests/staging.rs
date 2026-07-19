//! `staging` group of the signal_prepare tests (split from the former
//! monolithic `tests.rs`; test names unchanged).

use crate::signal_prepare::{
    SimpleSigType, prepare_signals_for_fir, prepare_signals_for_fir_verified,
};
use signals::{BlockRevPolicy, SigBuilder, SigMatch, match_sig};
use tlib::{de_bruijn_rec, de_bruijn_ref, list_to_vec, match_sym_rec, match_sym_ref};

#[test]
fn prepare_signals_for_fir_converts_shared_debruijn_group_once_per_forest() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let feedback = b.proj(0, self_ref);
        b.add(feedback, in0)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let (proj0, proj1) = {
        let mut b = SigBuilder::new(&mut arena);
        let proj0 = b.proj(0, group);
        let proj1 = b.proj(0, group);
        (proj0, proj1)
    };

    let prepared = prepare_signals_for_fir(&arena, &[proj0, proj1], &ui::UiProgram::empty())
        .expect("closed recursion group");

    assert_eq!(prepared.outputs.len(), 2);
    let SigMatch::Proj(_, left_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("expected left projection");
    };
    let SigMatch::Proj(_, right_group) = match_sig(&prepared.arena, prepared.outputs[1]) else {
        panic!("expected right projection");
    };
    assert_eq!(
        left_group, right_group,
        "forest preparation should keep one symbolic group identity across outputs"
    );

    let (var, body_list) =
        match_sym_rec(&prepared.arena, left_group).expect("symbolic recursion expected");
    let body = prepared
        .arena
        .hd(body_list)
        .expect("symbolic body list head");
    let SigMatch::BinOp(_, lhs, rhs) = match_sig(&prepared.arena, body) else {
        panic!("prepared recursive body should stay intact");
    };
    let (feedback_group, input_side) = match (
        match_sig(&prepared.arena, lhs),
        match_sig(&prepared.arena, rhs),
    ) {
        (SigMatch::Proj(0, feedback_group), SigMatch::Input(0)) => (feedback_group, rhs),
        (SigMatch::Input(0), SigMatch::Proj(0, feedback_group)) => (feedback_group, lhs),
        _ => panic!("prepared recursive body should keep one input and one proj(0, symref(var))"),
    };
    assert_eq!(match_sym_ref(&prepared.arena, feedback_group), Some(var));
    assert_eq!(match_sig(&prepared.arena, input_side), SigMatch::Input(0));
    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
}
#[test]
fn prepare_signals_for_fir_accepts_filter_carrier_children() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let x = b.input(0);
        let c0 = b.real(1.0);
        let c1 = b.real(-0.5);
        b.fir(&[x, c0, c1])
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("FIR carrier should prepare");

    let SigMatch::Fir(coefs) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should keep FIR carrier");
    };
    assert_eq!(coefs.len(), 3);
    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
}
#[test]
fn prepare_signals_for_fir_simplifies_algebraic_identities_before_fir() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let input = b.input(0);
        let zero = b.int(0);
        b.add(input, zero)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("algebraic simplification should succeed");

    assert_eq!(
        match_sig(&prepared.arena, prepared.outputs[0]),
        SigMatch::Input(0),
        "prepare_signals_for_fir should simplify x + 0 before FIR lowering"
    );
}
#[test]
fn prepare_signals_for_fir_canonicalizes_unary_recursive_projection_indices() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(7, self_ref);
        b.delay1(feedback)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(7, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("degenerate recursive projection should prepare");

    let SigMatch::Proj(0, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should canonicalize to proj(0, ...)");
    };
    let (_, prepared_body_list) =
        match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
    let prepared_body = prepared
        .arena
        .hd(prepared_body_list)
        .expect("prepared recursion body head");
    let SigMatch::Delay1(feedback) = match_sig(&prepared.arena, prepared_body) else {
        panic!("prepared body should canonicalize to Delay1");
    };
    let SigMatch::Proj(0, feedback_group) = match_sig(&prepared.arena, feedback) else {
        panic!("feedback edge should canonicalize to proj(0, symref(var))");
    };
    let (var, _) =
        match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
    assert_eq!(match_sym_ref(&prepared.arena, feedback_group), Some(var));
}
#[test]
fn prepare_signals_for_fir_canonicalizes_literal_delay_one_to_delay1() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let input = b.input(0);
        let one = b.int(1);
        b.delay(input, one)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("literal one-sample delay should prepare");

    let SigMatch::Delay1(inner) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should canonicalize to Delay1");
    };
    assert_eq!(match_sig(&prepared.arena, inner), SigMatch::Input(0));
}
#[test]
fn prepare_signals_for_fir_handles_shared_unary_recursion_dag_linearly() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(7, self_ref);
        b.delay1(feedback)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let leaf = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(7, group)
    };
    let mut shared = leaf;
    for _ in 0..24 {
        let mut b = SigBuilder::new(&mut arena);
        shared = b.add(shared, shared);
    }

    let prepared = prepare_signals_for_fir(&arena, &[shared], &ui::UiProgram::empty())
        .expect("shared unary recursion dag should prepare");

    assert!(
        prepared.outputs[0].as_u32() != 0,
        "preparation should produce a staged output"
    );
}
#[test]
fn prepare_signals_for_fir_verified_exposes_checked_staging_boundary() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.input(0)
    };

    let prepared = prepare_signals_for_fir_verified(&arena, &[output], &ui::UiProgram::empty())
        .expect("verified preparation should succeed");

    assert_eq!(prepared.outputs().len(), 1);
    assert_eq!(
        prepared.ty(prepared.outputs()[0]),
        Some(SimpleSigType::Real)
    );
    assert!(prepared.sig_ty(prepared.outputs()[0]).is_some());
}
#[test]
fn prepare_signals_for_fir_preserves_block_reverse_ad_carrier() {
    // Phase B0 invariant: a `BlockReverseAD` carrier projected for its
    // primal output must survive `prepare_signals_for_fir` end-to-end and
    // keep its layout and policy.
    let mut arena = tlib::TreeArena::new();
    let (carrier, primal_proj) = {
        let mut b = SigBuilder::new(&mut arena);
        let x = b.input(0);
        let y = b.input(1);
        let primal = b.add(x, y);
        let one = b.real(1.0);
        let carrier = b.block_reverse_ad(&[primal], &[x, y], &[one], BlockRevPolicy::TapeFull);
        let primal_proj = b.proj(0, carrier);
        (carrier, primal_proj)
    };

    let prepared = prepare_signals_for_fir(&arena, &[primal_proj], &ui::UiProgram::empty())
        .expect("BlockReverseAD primal projection should pass preparation");

    let SigMatch::Proj(0, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should remain a Proj");
    };
    let SigMatch::BlockReverseAD {
        body,
        primal_count,
        seeds,
        cotangents,
        policy,
    } = match_sig(&prepared.arena, prepared_group)
    else {
        panic!("Proj target should still be a BlockReverseAD carrier");
    };
    assert_eq!(primal_count, 1);
    assert_eq!(policy, BlockRevPolicy::TapeFull);
    assert_eq!(
        list_to_vec(&prepared.arena, body).expect("body list").len(),
        1
    );
    assert_eq!(
        list_to_vec(&prepared.arena, seeds)
            .expect("seed list")
            .len(),
        2
    );
    assert_eq!(
        list_to_vec(&prepared.arena, cotangents)
            .expect("cotangent list")
            .len(),
        1
    );
    let _ = carrier; // silence unused-binding lint when the helper is read-only
}
