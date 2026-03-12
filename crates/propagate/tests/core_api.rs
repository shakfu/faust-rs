//! Integration tests for `core_api`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use boxes::{BoxBuilder, BoxMatch, match_box};
use errors::{IntoDiagnostic, Severity, Stage, codes};
use propagate::{
    ArityCache, FlatBoxBuildError, PropagateError, box_arity, box_arity_typed, make_sig_input_list,
    propagate, propagate_typed, propagate_typed_with_ui, propagate_with_ui, try_build_flat_box,
};
use signals::{BinOp, SigBuilder, SigMatch, match_sig};
use tlib::{NodeKind, TreeArena, TreeId};
use ui::{ControlKind, UiGroupKind, UiMatch, UiRootOrigin, match_ui};

#[test]
fn make_sig_input_list_builds_ordered_inputs() {
    let mut arena = TreeArena::new();
    let sigs = make_sig_input_list(&mut arena, 4);
    assert_eq!(sigs.len(), 4);
    assert_eq!(match_sig(&arena, sigs[0]), SigMatch::Input(0));
    assert_eq!(match_sig(&arena, sigs[1]), SigMatch::Input(1));
    assert_eq!(match_sig(&arena, sigs[2]), SigMatch::Input(2));
    assert_eq!(match_sig(&arena, sigs[3]), SigMatch::Input(3));
}

#[test]
fn propagate_add_maps_to_signal_binop() {
    let mut arena = TreeArena::new();
    let add = BoxBuilder::new(&mut arena).add();
    let inputs = make_sig_input_list(&mut arena, 2);
    let out =
        propagate(&mut arena, add, &inputs, &mut ArityCache::new()).expect("add should propagate");
    assert_eq!(out.len(), 1);
    assert_eq!(
        match_sig(&arena, out[0]),
        SigMatch::BinOp(BinOp::Add, inputs[0], inputs[1])
    );
}

#[test]
fn propagate_seq_par_and_split_composition() {
    let mut arena = TreeArena::new();
    let (seq, split) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        let pair = bb.par(wire, wire);
        let add = bb.add();
        let seq = bb.seq(pair, add);
        let split = bb.split(wire, add);
        (seq, split)
    };
    let arity_seq = box_arity(&arena, seq, &mut ArityCache::new()).expect("seq arity should infer");
    assert_eq!(arity_seq.inputs, 2);
    assert_eq!(arity_seq.outputs, 1);

    let seq_inputs = make_sig_input_list(&mut arena, 2);
    let seq_out = propagate(&mut arena, seq, &seq_inputs, &mut ArityCache::new())
        .expect("seq should propagate");
    assert_eq!(
        match_sig(&arena, seq_out[0]),
        SigMatch::BinOp(BinOp::Add, seq_inputs[0], seq_inputs[1])
    );

    let split_inputs = make_sig_input_list(&mut arena, 1);
    let split_out = propagate(&mut arena, split, &split_inputs, &mut ArityCache::new())
        .expect("split should propagate");
    assert_eq!(
        match_sig(&arena, split_out[0]),
        SigMatch::BinOp(BinOp::Add, split_inputs[0], split_inputs[0])
    );
}

#[test]
fn propagate_merge_mixes_buses_before_right_box() {
    let mut arena = TreeArena::new();
    let merge = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        let two = bb.par(wire, wire);
        let four = bb.par(two, two);
        let add = bb.add();
        bb.merge(four, add)
    };
    let inputs = make_sig_input_list(&mut arena, 4);

    let out = propagate(&mut arena, merge, &inputs, &mut ArityCache::new())
        .expect("merge should propagate");
    assert_eq!(out.len(), 1);

    let SigMatch::BinOp(BinOp::Add, lhs, rhs) = match_sig(&arena, out[0]) else {
        panic!("merge output should be one add");
    };
    assert!(matches!(
        match_sig(&arena, lhs),
        SigMatch::BinOp(BinOp::Add, _, _)
    ));
    assert!(matches!(
        match_sig(&arena, rhs),
        SigMatch::BinOp(BinOp::Add, _, _)
    ));
}

#[test]
fn propagate_reports_arity_mismatch_and_supports_rec() {
    let mut arena = TreeArena::new();
    let bad_seq = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        let add = bb.add();
        bb.seq(wire, add)
    };
    let sig0 = SigBuilder::new(&mut arena).input(0);
    let err = propagate(&mut arena, bad_seq, &[sig0], &mut ArityCache::new())
        .expect_err("bad seq must fail");
    assert!(matches!(err, PropagateError::SeqArityMismatch { .. }));

    let rec = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        bb.rec(wire, wire)
    };
    let rec_arity = box_arity(&arena, rec, &mut ArityCache::new()).expect("rec arity should infer");
    assert_eq!(rec_arity.inputs, 0);
    assert_eq!(rec_arity.outputs, 1);

    let rec_out =
        propagate(&mut arena, rec, &[], &mut ArityCache::new()).expect("rec should propagate");
    assert_eq!(rec_out.len(), 1);
    let SigMatch::Proj(0, group) = match_sig(&arena, rec_out[0]) else {
        panic!("rec output should be proj(0, group)");
    };
    assert!(is_debruijn_rec(&arena, group));
}

#[test]
fn propagate_rec_plus_tilde_wire_shape_is_stable() {
    let mut arena = TreeArena::new();
    let rec = {
        let mut bb = BoxBuilder::new(&mut arena);
        let add = bb.add();
        let wire = bb.wire();
        bb.rec(add, wire)
    };
    let inputs = make_sig_input_list(&mut arena, 1);
    let out = propagate(&mut arena, rec, &inputs, &mut ArityCache::new())
        .expect("rec +~_ should propagate");
    assert_eq!(out.len(), 1);

    let SigMatch::Proj(0, group) = match_sig(&arena, out[0]) else {
        panic!("expected proj output");
    };
    let body_list = debruijn_body(&arena, group).expect("group should be debruijn(rec-body)");
    let first = arena
        .hd(body_list)
        .expect("rec body should have one branch");
    let SigMatch::BinOp(BinOp::Add, a, b) = match_sig(&arena, first) else {
        panic!("rec body branch should be add");
    };
    assert_eq!(match_sig(&arena, b), SigMatch::Input(0));
    let SigMatch::Delay1(d) = match_sig(&arena, a) else {
        panic!("first add argument should be delay1(proj(...))");
    };
    let SigMatch::Proj(0, seed_ref) = match_sig(&arena, d) else {
        panic!("delay1 arg should be proj(0, DEBRUIJNREF(1))");
    };
    assert!(is_debruijn_ref_level1(&arena, seed_ref));
}

#[test]
fn propagate_rec_keeps_closed_branches_outside_projection() {
    let mut arena = TreeArena::new();
    let rec = {
        let mut bb = BoxBuilder::new(&mut arena);
        let cst = bb.int(7);
        let add = bb.add();
        let left = bb.par(cst, add);
        let right = bb.wire();
        bb.rec(left, right)
    };
    let inputs = make_sig_input_list(&mut arena, 1);
    let out = propagate(&mut arena, rec, &inputs, &mut ArityCache::new())
        .expect("mixed rec should propagate");
    assert_eq!(out.len(), 2);
    assert_eq!(match_sig(&arena, out[0]), SigMatch::Int(7));
    assert!(matches!(match_sig(&arena, out[1]), SigMatch::Proj(1, _)));
}

#[test]
fn inputs_outputs_boxes_lower_to_signal_ints() {
    let mut arena = TreeArena::new();
    let (inputs_box, outputs_box) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        let add = bb.add();
        let par = bb.par(wire, add);
        let inputs_box = bb.inputs(par);
        let outputs_box = bb.outputs(par);
        (inputs_box, outputs_box)
    };

    let iout = propagate(&mut arena, inputs_box, &[], &mut ArityCache::new())
        .expect("inputs(...) should propagate");
    let oout = propagate(&mut arena, outputs_box, &[], &mut ArityCache::new())
        .expect("outputs(...) should propagate");

    assert_eq!(match_sig(&arena, iout[0]), SigMatch::Int(3));
    assert_eq!(match_sig(&arena, oout[0]), SigMatch::Int(2));

    assert!(matches!(match_box(&arena, inputs_box), BoxMatch::Inputs(_)));
    assert!(matches!(
        match_box(&arena, outputs_box),
        BoxMatch::Outputs(_)
    ));
}

#[test]
fn waveform_box_lowers_to_size_and_waveform_signal() {
    let mut arena = TreeArena::new();
    let waveform = {
        let mut bb = BoxBuilder::new(&mut arena);
        let v0 = bb.int(1);
        let v1 = bb.int(-2);
        let v2 = bb.real(3.5);
        bb.waveform(&[v0, v1, v2])
    };

    let arity =
        box_arity(&arena, waveform, &mut ArityCache::new()).expect("waveform arity should infer");
    assert_eq!(arity.inputs, 0);
    assert_eq!(arity.outputs, 2);

    let out = propagate(&mut arena, waveform, &[], &mut ArityCache::new())
        .expect("waveform should propagate");
    assert_eq!(out.len(), 2);
    assert_eq!(match_sig(&arena, out[0]), SigMatch::Int(3));

    let SigMatch::Waveform(values) = match_sig(&arena, out[1]) else {
        panic!("second output should be SIGWAVEFORM");
    };
    assert_eq!(values.len(), 3);
    assert!(matches!(match_sig(&arena, values[0]), SigMatch::Int(1)));
    assert!(matches!(match_sig(&arena, values[1]), SigMatch::Int(-2)));
    assert!(matches!(match_sig(&arena, values[2]), SigMatch::Real(_)));
}

fn expect_ui_group(
    program: &ui::UiProgram,
    node: ui::UiId,
    expected_kind: UiGroupKind,
    expected_label: &str,
) -> Vec<ui::UiId> {
    let UiMatch::Group {
        kind,
        label,
        children,
    } = match_ui(&program.arena, node)
    else {
        panic!("expected UI group node");
    };
    assert_eq!(kind, expected_kind);
    assert_eq!(label, expected_label);
    children
}

#[test]
fn propagate_with_ui_collects_nested_groups_and_control_specs() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let label_main = bb.ident("main");
        let label_mix = bb.ident("mix");
        let label_gain = bb.ident("gain");
        let init = bb.real(0.5);
        let min = bb.real(0.0);
        let max = bb.real(1.0);
        let step = bb.real(0.01);
        let slider = bb.hslider(label_gain, init, min, max, step);
        let grouped = bb.hgroup(label_mix, slider);
        bb.vgroup(label_main, grouped)
    };

    let out = propagate_with_ui(&mut arena, process, &[], &mut ArityCache::new())
        .expect("grouped slider should propagate with UI");

    assert_eq!(out.signals.len(), 1);
    assert_eq!(out.ui.root_origin, UiRootOrigin::Explicit);
    assert!(matches!(
        match_sig(&arena, out.signals[0]),
        SigMatch::HSlider(_)
    ));
    let outer = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "main");
    assert_eq!(outer.len(), 1);
    let inner = expect_ui_group(&out.ui, outer[0], UiGroupKind::Horizontal, "mix");
    assert_eq!(inner.len(), 1);
    assert_eq!(match_ui(&out.ui.arena, inner[0]), UiMatch::InputControl(0));
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::HSlider);
    assert_eq!(out.ui.controls[0].label, "gain");
    let range = out.ui.controls[0]
        .range
        .expect("slider range should be preserved");
    assert_eq!(range.init, 0.5);
    assert_eq!(range.min, 0.0);
    assert_eq!(range.max, 1.0);
    assert_eq!(range.step, 0.01);
}

#[test]
fn propagate_with_ui_synthesizes_root_group_for_multiple_ui_roots() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let left_label = bb.ident("main");
        let left_control_label = bb.ident("a");
        let left_control = bb.checkbox(left_control_label);
        let left = bb.vgroup(left_label, left_control);
        let right_label = bb.ident("top");
        let right_control_label = bb.ident("b");
        let right_control = bb.button(right_control_label);
        let right = bb.hgroup(right_label, right_control);
        bb.par(left, right)
    };

    let out = propagate_with_ui(&mut arena, process, &[], &mut ArityCache::new())
        .expect("multiple grouped UI roots should propagate");

    assert_eq!(out.ui.root_origin, UiRootOrigin::Synthesized);
    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "");
    assert_eq!(root_children.len(), 2);
    let left_children = expect_ui_group(&out.ui, root_children[0], UiGroupKind::Vertical, "main");
    let right_children = expect_ui_group(&out.ui, root_children[1], UiGroupKind::Horizontal, "top");
    assert_eq!(left_children.len(), 1);
    assert_eq!(right_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, left_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(
        match_ui(&out.ui.arena, right_children[0]),
        UiMatch::InputControl(1)
    );
    assert_eq!(out.ui.controls.len(), 2);
    assert_eq!(out.ui.controls[0].kind, ControlKind::Checkbox);
    assert_eq!(out.ui.controls[0].label, "a");
    assert_eq!(out.ui.controls[1].kind, ControlKind::Button);
    assert_eq!(out.ui.controls[1].label, "b");
}

#[test]
fn soundfile_box_lowers_to_length_rate_and_channel_buffers() {
    let mut arena = TreeArena::new();
    let soundfile = {
        let mut bb = BoxBuilder::new(&mut arena);
        let label = bb.ident("sf");
        let chan = bb.int(2);
        bb.soundfile(label, chan)
    };
    let inputs = make_sig_input_list(&mut arena, 2);

    let out = propagate(&mut arena, soundfile, &inputs, &mut ArityCache::new())
        .expect("soundfile should propagate");
    assert_eq!(out.len(), 4);

    let sf_sig = match match_sig(&arena, out[0]) {
        SigMatch::SoundfileLength(soundfile, part) => {
            assert_eq!(part, inputs[0]);
            soundfile
        }
        other => panic!("expected SoundfileLength, got {other:?}"),
    };
    assert_eq!(
        match_sig(&arena, out[1]),
        SigMatch::SoundfileRate(sf_sig, inputs[0])
    );

    let SigMatch::SoundfileBuffer(sf0, chan0, part0, ridx0) = match_sig(&arena, out[2]) else {
        panic!("first channel should be SoundfileBuffer");
    };
    assert_eq!(sf0, sf_sig);
    assert_eq!(part0, inputs[0]);
    assert_eq!(match_sig(&arena, chan0), SigMatch::Int(0));
    assert!(matches!(match_sig(&arena, ridx0), SigMatch::Max(_, _)));

    let SigMatch::SoundfileBuffer(sf1, chan1, part1, ridx1) = match_sig(&arena, out[3]) else {
        panic!("second channel should be SoundfileBuffer");
    };
    assert_eq!(sf1, sf_sig);
    assert_eq!(part1, inputs[0]);
    assert_eq!(ridx1, ridx0);
    assert_eq!(match_sig(&arena, chan1), SigMatch::Int(1));
}

#[test]
fn clocked_wrapper_boxes_port_trivial_and_structural_cases() {
    let mut arena = TreeArena::new();
    let (ondemand, upsampling, downsampling) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire0 = bb.wire();
        let ondemand = bb.ondemand(wire0);
        let wire1 = bb.wire();
        let upsampling = bb.upsampling(wire1);
        let wire2 = bb.wire();
        let downsampling = bb.downsampling(wire2);
        (ondemand, upsampling, downsampling)
    };

    let zero = SigBuilder::new(&mut arena).int(0);
    let one = SigBuilder::new(&mut arena).int(1);
    let x = SigBuilder::new(&mut arena).input(7);
    let h = SigBuilder::new(&mut arena).input(3);

    let od_zero = propagate(&mut arena, ondemand, &[zero, x], &mut ArityCache::new())
        .expect("ondemand zero clock should propagate");
    assert_eq!(od_zero, vec![zero]);

    let od_one = propagate(&mut arena, ondemand, &[one, x], &mut ArityCache::new())
        .expect("ondemand one clock should bypass wrapper");
    assert_eq!(od_one, vec![x]);

    let od = propagate(&mut arena, ondemand, &[h, x], &mut ArityCache::new())
        .expect("ondemand dynamic clock should propagate");
    let SigMatch::Seq(od_wrapper, od_payload) = match_sig(&arena, od[0]) else {
        panic!("ondemand output should be seq(wrapper, payload)");
    };
    assert!(matches!(
        match_sig(&arena, od_wrapper),
        SigMatch::OnDemand(_)
    ));
    assert!(matches!(
        match_sig(&arena, od_payload),
        SigMatch::PermVar(_)
    ));

    let us = propagate(&mut arena, upsampling, &[h, x], &mut ArityCache::new())
        .expect("upsampling dynamic clock should propagate");
    let SigMatch::Seq(us_wrapper, us_payload) = match_sig(&arena, us[0]) else {
        panic!("upsampling output should be seq(wrapper, payload)");
    };
    assert!(matches!(
        match_sig(&arena, us_wrapper),
        SigMatch::Upsampling(_)
    ));
    assert!(matches!(
        match_sig(&arena, us_payload),
        SigMatch::PermVar(_)
    ));

    let ds = propagate(&mut arena, downsampling, &[h, x], &mut ArityCache::new())
        .expect("downsampling dynamic clock should propagate");
    let SigMatch::Seq(ds_wrapper, ds_payload) = match_sig(&arena, ds[0]) else {
        panic!("downsampling output should be seq(wrapper, payload)");
    };
    assert!(matches!(
        match_sig(&arena, ds_wrapper),
        SigMatch::Downsampling(_)
    ));
    assert!(matches!(
        match_sig(&arena, ds_payload),
        SigMatch::PermVar(_)
    ));
}

#[test]
fn route_box_propagates_by_mixing_selected_inputs() {
    let mut arena = TreeArena::new();
    let route = {
        let mut bb = BoxBuilder::new(&mut arena);
        let two = bb.int(2);
        let one_a = bb.int(1);
        let one_b = bb.int(1);
        let two_a = bb.int(2);
        let one_c = bb.int(1);
        let left_pair = bb.par(one_a, one_b);
        let right_pair = bb.par(two_a, one_c);
        let spec = bb.par(left_pair, right_pair);
        bb.route(two, two, spec)
    };
    let inputs = make_sig_input_list(&mut arena, 2);

    let arity = box_arity(&arena, route, &mut ArityCache::new()).expect("route arity should infer");
    assert_eq!(arity.inputs, 2);
    assert_eq!(arity.outputs, 2);

    let out = propagate(&mut arena, route, &inputs, &mut ArityCache::new())
        .expect("route should propagate");
    assert_eq!(out.len(), 2);
    let zero = SigBuilder::new(&mut arena).int(0);
    let SigMatch::BinOp(BinOp::Add, partial, rhs) = match_sig(&arena, out[0]) else {
        panic!("first route output should be an add");
    };
    assert_eq!(rhs, inputs[1]);
    assert_eq!(
        match_sig(&arena, partial),
        SigMatch::BinOp(BinOp::Add, zero, inputs[0])
    );
    assert_eq!(match_sig(&arena, out[1]), SigMatch::Int(0));
}

#[test]
fn ffun_box_arity_and_propagation_follow_signature() {
    let mut arena = TreeArena::new();
    let (wrapped, ff) = {
        let ty = arena.int(1);
        let incfile = arena.symbol("<math.h>");
        let libfile = arena.symbol("\"\"");
        let cname = arena.symbol("myfun");
        let nil = arena.nil();
        let names_tail = arena.cons(cname, nil);
        let names_mid = arena.cons(cname, names_tail);
        let names_mid2 = arena.cons(cname, names_mid);
        let names = arena.cons(cname, names_mid2);
        let arg_types = arena.cons(ty, nil);
        let payload = arena.cons(names, arg_types);
        let signature = arena.cons(ty, payload);
        let mut bb = BoxBuilder::new(&mut arena);
        let ff = bb.ffunction(signature, incfile, libfile);
        (bb.ffun(ff), ff)
    };
    let inputs = make_sig_input_list(&mut arena, 1);

    let arity =
        box_arity(&arena, wrapped, &mut ArityCache::new()).expect("ffun arity should infer");
    assert_eq!(arity.inputs, 1);
    assert_eq!(arity.outputs, 1);

    let out = propagate(&mut arena, wrapped, &inputs, &mut ArityCache::new())
        .expect("ffun should propagate");
    assert_eq!(out.len(), 1);
    let SigMatch::FFun(sig_ff, largs) = match_sig(&arena, out[0]) else {
        panic!("ffun output should be SIGFFUN");
    };
    assert_eq!(sig_ff, ff);
    assert_eq!(arena.hd(largs), Some(inputs[0]));
    assert!(arena.tl(largs).is_some_and(|tail| arena.is_nil(tail)));
}

#[test]
fn flat_box_builder_accepts_valid_post_eval_families() {
    let mut arena = TreeArena::new();
    let valid = {
        let mut bb = BoxBuilder::new(&mut arena);
        let slot = bb.slot(0);
        let wire_for_symbolic = bb.wire();
        let symbolic = bb.symbolic(slot, wire_for_symbolic);
        let route_in = bb.int(1);
        let route_out = bb.int(1);
        let route_src = bb.int(1);
        let route_dst = bb.int(1);
        let route_spec = bb.par(route_src, route_dst);
        let route = bb.route(route_in, route_out, route_spec);
        let ondemand_body = bb.wire();
        let ondemand = bb.ondemand(ondemand_body);
        let upsampling_body = bb.wire();
        let upsampling = bb.upsampling(upsampling_body);
        let downsampling_body = bb.wire();
        let downsampling = bb.downsampling(downsampling_body);
        let sf_label = bb.ident("sf");
        let sf_chan = bb.int(2);
        let soundfile = bb.soundfile(sf_label, sf_chan);
        let environment = bb.environment();
        let md_key = bb.ident("k");
        let md_value = bb.ident("v");
        let md_list = bb.par(md_key, md_value);
        vec![
            bb.metadata(symbolic, md_list),
            route,
            ondemand,
            upsampling,
            downsampling,
            soundfile,
            environment,
        ]
    };

    for node in valid {
        try_build_flat_box(&arena, node).expect("node should belong to flat post-eval subset");
    }
}

#[test]
fn flat_box_builder_rejects_evaluator_only_families() {
    let mut arena = TreeArena::new();
    let bad = {
        let mut bb = BoxBuilder::new(&mut arena);
        let ident = bb.ident("x");
        let rule_l = bb.wire();
        let rule_r = bb.wire();
        let rules = bb.par(rule_l, rule_r);
        let abstr_body = bb.wire();
        let modulation_body = bb.wire();
        let with_lhs = bb.wire();
        let with_rhs = bb.wire();
        let modif_lhs = bb.wire();
        let modif_rhs = bb.wire();
        let withrec_a = bb.wire();
        let withrec_b = bb.wire();
        let withrec_c = bb.wire();
        let appl_fun = bb.wire();
        let appl_arg = bb.wire();
        let access_expr = bb.wire();
        let ipar_count = bb.int(2);
        let ipar_body = bb.wire();
        let iseq_count = bb.int(2);
        let iseq_body = bb.wire();
        let isum_count = bb.int(2);
        let isum_body = bb.wire();
        let iprod_count = bb.int(2);
        let iprod_body = bb.wire();
        let ff_sig = bb.wire();
        let ff_inc = bb.wire();
        let ff_lib = bb.wire();
        vec![
            (bb.case(rules), "case"),
            (bb.pattern_var(ident), "patternvar"),
            (bb.abstr(ident, abstr_body), "abstr"),
            (bb.modulation(ident, modulation_body), "modulation"),
            (bb.with_local_def(with_lhs, with_rhs), "withlocaldef"),
            (bb.modif_local_def(modif_lhs, modif_rhs), "modiflocaldef"),
            (
                bb.with_rec_def(withrec_a, withrec_b, withrec_c),
                "withrecdef",
            ),
            (bb.component(ident), "component"),
            (bb.library(ident), "library"),
            (bb.appl(appl_fun, appl_arg), "appl"),
            (bb.access(access_expr, ident), "access"),
            (bb.ipar(ident, ipar_count, ipar_body), "ipar"),
            (bb.iseq(ident, iseq_count, iseq_body), "iseq"),
            (bb.isum(ident, isum_count, isum_body), "isum"),
            (bb.iprod(ident, iprod_count, iprod_body), "iprod"),
            (bb.ffunction(ff_sig, ff_inc, ff_lib), "ffunction"),
        ]
    };

    for (node, kind) in bad {
        let err = try_build_flat_box(&arena, node).expect_err("node must be rejected");
        assert_eq!(err, FlatBoxBuildError::UnexpectedPostEvalBox { node, kind });
    }
}

#[test]
fn flat_box_builder_rejects_nested_non_flat_subtrees() {
    let mut arena = TreeArena::new();
    let (seq, nested_bad) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let bad_l = bb.wire();
        let bad_r = bb.wire();
        let bad_rules = bb.par(bad_l, bad_r);
        let nested_bad = bb.case(bad_rules);
        let rhs = bb.wire();
        (bb.seq(nested_bad, rhs), nested_bad)
    };

    let err = try_build_flat_box(&arena, seq).expect_err("nested case must be rejected");
    assert_eq!(
        err,
        FlatBoxBuildError::UnexpectedPostEvalBox {
            node: nested_bad,
            kind: "case",
        }
    );
}

#[test]
fn box_arity_typed_uses_validated_flat_boundary() {
    let mut arena = TreeArena::new();
    let (seq, bad_case) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire_l = bb.wire();
        let wire_r = bb.wire();
        let pair = bb.par(wire_l, wire_r);
        let add = bb.add();
        let seq = bb.seq(pair, add);

        let case_l = bb.wire();
        let case_r = bb.wire();
        let rules = bb.par(case_l, case_r);
        let bad_case = bb.case(rules);
        (seq, bad_case)
    };

    let flat = try_build_flat_box(&arena, seq).expect("seq should validate as flat");
    let arity =
        box_arity_typed(&arena, flat, &mut ArityCache::new()).expect("typed arity should work");
    assert_eq!(arity.inputs, 2);
    assert_eq!(arity.outputs, 1);

    let err = box_arity(&arena, bad_case, &mut ArityCache::new())
        .expect_err("case should be rejected before arity inference");
    assert_eq!(
        err,
        PropagateError::UnsupportedBox {
            node: bad_case,
            kind: "case",
        }
    );
}

#[test]
fn propagate_typed_uses_flat_boundary_and_matches_wrapper() {
    let mut arena = TreeArena::new();
    let (seq, bad_case) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire_l = bb.wire();
        let wire_r = bb.wire();
        let pair = bb.par(wire_l, wire_r);
        let add = bb.add();
        let seq = bb.seq(pair, add);

        let case_l = bb.wire();
        let case_r = bb.wire();
        let rules = bb.par(case_l, case_r);
        let bad_case = bb.case(rules);
        (seq, bad_case)
    };

    let flat = try_build_flat_box(&arena, seq).expect("seq should validate as flat");
    let inputs = make_sig_input_list(&mut arena, 2);
    let typed_out = propagate_typed(&mut arena, flat, &inputs, &mut ArityCache::new())
        .expect("typed propagation should succeed");
    let typed_with_ui = propagate_typed_with_ui(&mut arena, flat, &inputs, &mut ArityCache::new())
        .expect("typed propagation with UI should succeed");
    let raw_out = propagate(&mut arena, seq, &inputs, &mut ArityCache::new())
        .expect("wrapper should succeed");
    assert_eq!(typed_out, raw_out);
    assert_eq!(typed_with_ui.signals, raw_out);

    let err = propagate(&mut arena, bad_case, &[], &mut ArityCache::new())
        .expect_err("case should be rejected before propagation");
    assert_eq!(
        err,
        PropagateError::UnsupportedBox {
            node: bad_case,
            kind: "case",
        }
    );
}

#[test]
fn propagate_pow_min_max_map_to_signal_nodes() {
    let mut arena = TreeArena::new();
    let (pow, min, max) = {
        let mut bb = BoxBuilder::new(&mut arena);
        (bb.pow(), bb.min(), bb.max())
    };
    let inputs = make_sig_input_list(&mut arena, 2);

    let pow_out =
        propagate(&mut arena, pow, &inputs, &mut ArityCache::new()).expect("pow should propagate");
    let min_out =
        propagate(&mut arena, min, &inputs, &mut ArityCache::new()).expect("min should propagate");
    let max_out =
        propagate(&mut arena, max, &inputs, &mut ArityCache::new()).expect("max should propagate");

    assert_eq!(
        match_sig(&arena, pow_out[0]),
        SigMatch::Pow(inputs[0], inputs[1])
    );
    assert_eq!(
        match_sig(&arena, min_out[0]),
        SigMatch::Min(inputs[0], inputs[1])
    );
    assert_eq!(
        match_sig(&arena, max_out[0]),
        SigMatch::Max(inputs[0], inputs[1])
    );
}

#[test]
fn slot_and_symbolic_boxes_propagate_like_cpp_placeholders() {
    let mut arena = TreeArena::new();
    let (slot7, passthrough, pair) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let slot7 = bb.slot(7);
        let slot1 = bb.slot(1);
        let slot2 = bb.slot(2);
        let passthrough = bb.symbolic(slot1, slot1);
        let pair_body = bb.par(slot1, slot2);
        let pair_inner = bb.symbolic(slot2, pair_body);
        let pair = bb.symbolic(slot1, pair_inner);
        (slot7, passthrough, pair)
    };

    let slot_arity =
        box_arity(&arena, slot7, &mut ArityCache::new()).expect("slot arity should infer");
    assert_eq!(slot_arity.inputs, 0);
    assert_eq!(slot_arity.outputs, 1);

    let fallback = propagate(&mut arena, slot7, &[], &mut ArityCache::new())
        .expect("raw slot should lower to deterministic dummy input");
    assert_eq!(match_sig(&arena, fallback[0]), SigMatch::Input(7));

    let inputs = make_sig_input_list(&mut arena, 2);
    let passthrough_out = propagate(
        &mut arena,
        passthrough,
        &inputs[..1],
        &mut ArityCache::new(),
    )
    .expect("symbolic(slot, slot) should forward its bound input");
    assert_eq!(passthrough_out, vec![inputs[0]]);

    let pair_arity = box_arity(&arena, pair, &mut ArityCache::new())
        .expect("nested symbolic arity should infer");
    assert_eq!(pair_arity.inputs, 2);
    assert_eq!(pair_arity.outputs, 2);

    let pair_out = propagate(&mut arena, pair, &inputs, &mut ArityCache::new())
        .expect("nested symbolic should preserve remaining inputs");
    assert_eq!(pair_out, inputs);
}

#[test]
fn propagate_extended_math_primitives_map_to_signal_nodes() {
    let mut arena = TreeArena::new();
    let (
        acos,
        asin,
        atan,
        atan2,
        cos,
        sin,
        tan,
        exp,
        log,
        log10,
        sqrt,
        abs,
        fmod,
        remainder,
        floor,
        ceil,
        rint,
        round,
    ) = {
        let mut bb = BoxBuilder::new(&mut arena);
        (
            bb.acos(),
            bb.asin(),
            bb.atan(),
            bb.atan2(),
            bb.cos(),
            bb.sin(),
            bb.tan(),
            bb.exp(),
            bb.log(),
            bb.log10(),
            bb.sqrt(),
            bb.abs(),
            bb.fmod(),
            bb.remainder(),
            bb.floor(),
            bb.ceil(),
            bb.rint(),
            bb.round(),
        )
    };
    let uinputs = make_sig_input_list(&mut arena, 1);
    let binputs = make_sig_input_list(&mut arena, 2);

    let acos_sig = propagate(&mut arena, acos, &uinputs, &mut ArityCache::new())
        .expect("acos should propagate")[0];
    let asin_sig = propagate(&mut arena, asin, &uinputs, &mut ArityCache::new())
        .expect("asin should propagate")[0];
    let atan_sig = propagate(&mut arena, atan, &uinputs, &mut ArityCache::new())
        .expect("atan should propagate")[0];
    let atan2_sig = propagate(&mut arena, atan2, &binputs, &mut ArityCache::new())
        .expect("atan2 should propagate")[0];
    let cos_sig = propagate(&mut arena, cos, &uinputs, &mut ArityCache::new())
        .expect("cos should propagate")[0];
    let sin_sig = propagate(&mut arena, sin, &uinputs, &mut ArityCache::new())
        .expect("sin should propagate")[0];
    let tan_sig = propagate(&mut arena, tan, &uinputs, &mut ArityCache::new())
        .expect("tan should propagate")[0];
    let exp_sig = propagate(&mut arena, exp, &uinputs, &mut ArityCache::new())
        .expect("exp should propagate")[0];
    let log_sig = propagate(&mut arena, log, &uinputs, &mut ArityCache::new())
        .expect("log should propagate")[0];
    let log10_sig = propagate(&mut arena, log10, &uinputs, &mut ArityCache::new())
        .expect("log10 should propagate")[0];
    let sqrt_sig = propagate(&mut arena, sqrt, &uinputs, &mut ArityCache::new())
        .expect("sqrt should propagate")[0];
    let abs_sig = propagate(&mut arena, abs, &uinputs, &mut ArityCache::new())
        .expect("abs should propagate")[0];
    let fmod_sig = propagate(&mut arena, fmod, &binputs, &mut ArityCache::new())
        .expect("fmod should propagate")[0];
    let remainder_sig = propagate(&mut arena, remainder, &binputs, &mut ArityCache::new())
        .expect("remainder should propagate")[0];
    let floor_sig = propagate(&mut arena, floor, &uinputs, &mut ArityCache::new())
        .expect("floor should propagate")[0];
    let ceil_sig = propagate(&mut arena, ceil, &uinputs, &mut ArityCache::new())
        .expect("ceil should propagate")[0];
    let rint_sig = propagate(&mut arena, rint, &uinputs, &mut ArityCache::new())
        .expect("rint should propagate")[0];
    let round_sig = propagate(&mut arena, round, &uinputs, &mut ArityCache::new())
        .expect("round should propagate")[0];

    assert_eq!(match_sig(&arena, acos_sig), SigMatch::Acos(uinputs[0]));
    assert_eq!(match_sig(&arena, asin_sig), SigMatch::Asin(uinputs[0]));
    assert_eq!(match_sig(&arena, atan_sig), SigMatch::Atan(uinputs[0]));
    assert_eq!(
        match_sig(&arena, atan2_sig),
        SigMatch::Atan2(binputs[0], binputs[1])
    );
    assert_eq!(match_sig(&arena, cos_sig), SigMatch::Cos(uinputs[0]));
    assert_eq!(match_sig(&arena, sin_sig), SigMatch::Sin(uinputs[0]));
    assert_eq!(match_sig(&arena, tan_sig), SigMatch::Tan(uinputs[0]));
    assert_eq!(match_sig(&arena, exp_sig), SigMatch::Exp(uinputs[0]));
    assert_eq!(match_sig(&arena, log_sig), SigMatch::Log(uinputs[0]));
    assert_eq!(match_sig(&arena, log10_sig), SigMatch::Log10(uinputs[0]));
    assert_eq!(match_sig(&arena, sqrt_sig), SigMatch::Sqrt(uinputs[0]));
    assert_eq!(match_sig(&arena, abs_sig), SigMatch::Abs(uinputs[0]));
    assert_eq!(
        match_sig(&arena, fmod_sig),
        SigMatch::Fmod(binputs[0], binputs[1])
    );
    assert_eq!(
        match_sig(&arena, remainder_sig),
        SigMatch::Remainder(binputs[0], binputs[1])
    );
    assert_eq!(match_sig(&arena, floor_sig), SigMatch::Floor(uinputs[0]));
    assert_eq!(match_sig(&arena, ceil_sig), SigMatch::Ceil(uinputs[0]));
    assert_eq!(match_sig(&arena, rint_sig), SigMatch::Rint(uinputs[0]));
    assert_eq!(match_sig(&arena, round_sig), SigMatch::Round(uinputs[0]));
}

#[test]
fn propagate_error_converts_to_structured_diagnostic_codes() {
    let mut arena = TreeArena::new();
    let node = BoxBuilder::new(&mut arena).wire();

    let unsupported = PropagateError::UnsupportedBox {
        node,
        kind: "ident",
    }
    .into_diagnostic();
    assert_eq!(unsupported.severity, Severity::Error);
    assert_eq!(unsupported.stage, Stage::Propagate);
    assert_eq!(unsupported.code, codes::PROP_UNSUPPORTED_BOX);
    assert!(
        unsupported
            .notes
            .iter()
            .any(|n| n.starts_with("cause: encountered box node family")),
        "unsupported-box diagnostics should expose explicit cause note"
    );
    assert!(!unsupported.help.is_empty());

    let arity = PropagateError::InputArityMismatch {
        node,
        expected: 2,
        got: 1,
    }
    .into_diagnostic();
    assert_eq!(arity.code, codes::PROP_ARITY_MISMATCH);
    assert!(!arity.notes.is_empty());
    assert!(
        arity
            .notes
            .iter()
            .any(|n| n.starts_with("cause: propagated bus width differs")),
        "arity diagnostics should expose explicit cause note"
    );
    assert!(!arity.help.is_empty());

    let split = PropagateError::SplitArityMismatch {
        node,
        left_outputs: 2,
        right_inputs: 3,
    }
    .into_diagnostic();
    assert_eq!(split.code, codes::PROP_ARITY_MISMATCH);
    assert!(split.notes.iter().any(|n| n.contains("rule: split(A, B)")));
    assert!(
        split
            .notes
            .iter()
            .any(|n| n.contains("computed: 3 % 2 = 1"))
    );
    assert!(
        split
            .notes
            .iter()
            .any(|n| n.contains("suggested target: set inputs(B) to 4"))
    );
    assert!(!split.help.is_empty());
    assert!(
        split
            .help
            .iter()
            .any(|h| h.contains("for `A <: B`, enforce inputs(B) % outputs(A) == 0"))
    );

    let rec = PropagateError::RecArityMismatch {
        node,
        left_inputs: 1,
        left_outputs: 1,
        right_inputs: 2,
        right_outputs: 1,
    }
    .into_diagnostic();
    assert_eq!(rec.code, codes::PROP_RECURSION_MISMATCH);
    assert!(!rec.notes.is_empty());
    assert!(!rec.help.is_empty());
    assert!(
        rec.notes
            .iter()
            .any(|n| n.contains("suggested target: set outputs(A) >= 2 and inputs(A) >= 1"))
    );
    assert!(
        rec.help
            .iter()
            .any(|h| h.contains("for `A ~ B`, enforce inputs(B) <= outputs(A)"))
    );
}

fn is_debruijn_rec(arena: &TreeArena, id: TreeId) -> bool {
    matches!(tag_name(arena, id), Some("DEBRUIJN"))
}

fn debruijn_body(arena: &TreeArena, id: TreeId) -> Option<TreeId> {
    if !is_debruijn_rec(arena, id) {
        return None;
    }
    let [body] = arena.children(id)? else {
        return None;
    };
    Some(*body)
}

fn is_debruijn_ref_level1(arena: &TreeArena, id: TreeId) -> bool {
    if !matches!(tag_name(arena, id), Some("DEBRUIJNREF")) {
        return false;
    }
    let Some([level]) = arena.children(id) else {
        return false;
    };
    matches!(arena.kind(*level), Some(NodeKind::Int(1)))
}

fn tag_name(arena: &TreeArena, id: TreeId) -> Option<&str> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = &node.kind else {
        return None;
    };
    arena.tag_name(*tag_id)
}
