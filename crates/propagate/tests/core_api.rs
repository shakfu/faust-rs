//! Integration tests for `core_api`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use boxes::{BoxBuilder, BoxMatch, match_box};
use errors::{IntoDiagnostic, Severity, Stage, codes};
use propagate::{
    ArityCache, FlatBoxBuildError, PropagateError, PropagateUiOptions, box_arity_typed,
    make_sig_input_list, propagate_typed, propagate_typed_with_ui, propagate_typed_with_ui_options,
    try_build_flat_box,
};
use signals::{BinOp, SigBuilder, SigMatch, match_sig};
use tlib::{DEBRUIJNREC_TAG, NodeKind, TreeArena, TreeId};
use ui::{ControlKind, UiGroupKind, UiMatch, UiRootOrigin, match_ui};

fn parser_definition(arena: &mut TreeArena, name: TreeId, expr: TreeId) -> TreeId {
    let nil = arena.nil();
    let payload = arena.cons(nil, expr);
    arena.cons(name, payload)
}

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
    let flat_add = try_build_flat_box(&arena, add).unwrap();
    let out = propagate_typed(&mut arena, flat_add, &inputs, &mut ArityCache::new())
        .expect("add should propagate");
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
    let arity_seq = box_arity_typed(
        &arena,
        try_build_flat_box(&arena, seq).unwrap(),
        &mut ArityCache::new(),
    )
    .expect("seq arity should infer");
    assert_eq!(arity_seq.inputs, 2);
    assert_eq!(arity_seq.outputs, 1);

    let seq_inputs = make_sig_input_list(&mut arena, 2);
    let flat_seq = try_build_flat_box(&arena, seq).unwrap();
    let seq_out = propagate_typed(&mut arena, flat_seq, &seq_inputs, &mut ArityCache::new())
        .expect("seq should propagate");
    assert_eq!(
        match_sig(&arena, seq_out[0]),
        SigMatch::BinOp(BinOp::Add, seq_inputs[0], seq_inputs[1])
    );

    let split_inputs = make_sig_input_list(&mut arena, 1);
    let flat_split = try_build_flat_box(&arena, split).unwrap();
    let split_out = propagate_typed(
        &mut arena,
        flat_split,
        &split_inputs,
        &mut ArityCache::new(),
    )
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

    let flat_merge = try_build_flat_box(&arena, merge).unwrap();
    let out = propagate_typed(&mut arena, flat_merge, &inputs, &mut ArityCache::new())
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
    let flat_bad_seq = try_build_flat_box(&arena, bad_seq).unwrap();
    let err = propagate_typed(&mut arena, flat_bad_seq, &[sig0], &mut ArityCache::new())
        .expect_err("bad seq must fail");
    assert!(matches!(err, PropagateError::SeqArityMismatch { .. }));

    let rec = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        bb.rec(wire, wire)
    };
    let rec_arity = box_arity_typed(
        &arena,
        try_build_flat_box(&arena, rec).unwrap(),
        &mut ArityCache::new(),
    )
    .expect("rec arity should infer");
    assert_eq!(rec_arity.inputs, 0);
    assert_eq!(rec_arity.outputs, 1);

    let flat_rec = try_build_flat_box(&arena, rec).unwrap();
    let rec_out = propagate_typed(&mut arena, flat_rec, &[], &mut ArityCache::new())
        .expect("rec should propagate");
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
    let flat_rec = try_build_flat_box(&arena, rec).unwrap();
    let out = propagate_typed(&mut arena, flat_rec, &inputs, &mut ArityCache::new())
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
    let flat_rec = try_build_flat_box(&arena, rec).unwrap();
    let out = propagate_typed(&mut arena, flat_rec, &inputs, &mut ArityCache::new())
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

    let flat_inputs_box = try_build_flat_box(&arena, inputs_box).unwrap();
    let iout = propagate_typed(&mut arena, flat_inputs_box, &[], &mut ArityCache::new())
        .expect("inputs(...) should propagate");
    let flat_outputs_box = try_build_flat_box(&arena, outputs_box).unwrap();
    let oout = propagate_typed(&mut arena, flat_outputs_box, &[], &mut ArityCache::new())
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

    let flat_waveform = try_build_flat_box(&arena, waveform).unwrap();
    let arity = box_arity_typed(&arena, flat_waveform, &mut ArityCache::new())
        .expect("waveform arity should infer");
    assert_eq!(arity.inputs, 0);
    assert_eq!(arity.outputs, 2);

    let out = propagate_typed(&mut arena, flat_waveform, &[], &mut ArityCache::new())
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
        metadata: _,
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

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
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
fn propagate_with_ui_extracts_group_and_widget_label_metadata() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let root_label = bb.ident("main [tooltip: use me]");
        let slider_label = bb.ident("gain [style:knob]");
        let init = bb.real(0.5);
        let min = bb.real(0.0);
        let max = bb.real(1.0);
        let step = bb.real(0.01);
        let slider = bb.hslider(slider_label, init, min, max, step);
        bb.vgroup(root_label, slider)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("metadata-bearing grouped slider should propagate");

    let UiMatch::Group {
        kind,
        label,
        metadata,
        children,
    } = match_ui(&out.ui.arena, out.ui.root)
    else {
        panic!("expected root UI group");
    };
    assert_eq!(kind, UiGroupKind::Vertical);
    assert_eq!(label, "main");
    assert_eq!(metadata, vec![("tooltip".to_owned(), "use me".to_owned())]);
    assert_eq!(children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "gain");
    assert_eq!(
        out.ui.controls[0].metadata,
        vec![("style".to_owned(), "knob".to_owned())]
    );
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

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
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
fn propagate_with_ui_options_assigns_canonical_root_label() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let root_label = bb.ident("");
        let checkbox_label = bb.ident("c");
        let checkbox = bb.checkbox(checkbox_label);
        let group = bb.vgroup(root_label, checkbox);
        try_build_flat_box(&arena, group).expect("root group should be flat")
    };

    let out = propagate_typed_with_ui_options(
        &mut arena,
        process,
        &[],
        &mut ArityCache::new(),
        &PropagateUiOptions::new("named-root"),
    )
    .expect("root-labeled grouped UI should propagate");

    assert_eq!(out.ui.root_origin, UiRootOrigin::Explicit);
    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "named-root");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
}

#[test]
fn propagate_with_ui_keeps_deterministic_control_order_across_mixed_nested_controls() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let root_label = bb.ident("root");
        let left_label = bb.ident("left");
        let gate_label = bb.ident("gate");
        let level_label = bb.ident("level");
        let level_init = bb.real(0.0);
        let level_min = bb.real(0.0);
        let level_max = bb.real(1.0);
        let level_step = bb.real(0.01);
        let tabs_label = bb.ident("tabs");
        let trigger_label = bb.ident("trigger");

        let gate = bb.checkbox(gate_label);
        let left = bb.hgroup(left_label, gate);
        let level = bb.hslider(level_label, level_init, level_min, level_max, level_step);
        let trigger = bb.button(trigger_label);
        let tabs = bb.tgroup(tabs_label, trigger);
        let row = bb.par(left, level);
        let content = bb.par(row, tabs);
        bb.vgroup(root_label, content)
    };
    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("mixed grouped UI should propagate");

    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "root");
    assert_eq!(root_children.len(), 3);
    let left_children = expect_ui_group(&out.ui, root_children[0], UiGroupKind::Horizontal, "left");
    assert_eq!(left_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, left_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(
        match_ui(&out.ui.arena, root_children[1]),
        UiMatch::InputControl(1)
    );
    let tabs_children = expect_ui_group(&out.ui, root_children[2], UiGroupKind::Tab, "tabs");
    assert_eq!(tabs_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, tabs_children[0]),
        UiMatch::InputControl(2)
    );

    assert_eq!(out.ui.controls.len(), 3);
    assert_eq!(out.ui.controls[0].kind, ControlKind::Checkbox);
    assert_eq!(out.ui.controls[0].label, "gate");
    assert_eq!(out.ui.controls[1].kind, ControlKind::HSlider);
    assert_eq!(out.ui.controls[1].label, "level");
    assert_eq!(out.ui.controls[2].kind, ControlKind::Button);
    assert_eq!(out.ui.controls[2].label, "trigger");
}

#[test]
fn propagate_with_ui_collects_soundfile_control_spec() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let root_label = bb.ident("files");
        let sound_label = bb.ident("sample[url:{'tests/assets/silence.wav'}]");
        let chan = bb.int(1);
        let sound = bb.soundfile(sound_label, chan);
        bb.vgroup(root_label, sound)
    };
    let inputs = make_sig_input_list(&mut arena, 2);

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &inputs, &mut ArityCache::new())
        .expect("soundfile grouped UI should propagate");

    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "files");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::Soundfile(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::Soundfile);
    assert_eq!(out.ui.controls[0].label, "sample");
    assert_eq!(
        out.ui.controls[0].metadata,
        vec![("url".to_owned(), "{'tests/assets/silence.wav'}".to_owned())]
    );
}

#[test]
fn propagate_with_ui_rebases_relative_widget_path_to_parent_group() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let foo = bb.ident("Foo");
        let faa = bb.ident("Faa");
        let volume = bb.ident("../volume");
        let init = bb.real(0.5);
        let min = bb.real(0.0);
        let max = bb.real(1.0);
        let step = bb.real(0.01);
        let slider = bb.hslider(volume, init, min, max, step);
        let inner = bb.vgroup(faa, slider);
        bb.hgroup(foo, inner)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("relative widget path should propagate");

    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Horizontal, "Foo");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "volume");
}

#[test]
fn propagate_with_ui_lowers_typed_widget_path_into_canonical_group() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let freq = bb.ident("h:Oscillator/freq");
        let init = bb.real(440.0);
        let min = bb.real(20.0);
        let max = bb.real(20_000.0);
        let step = bb.real(1.0);
        bb.hslider(freq, init, min, max, step)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("typed widget path should propagate");

    let root_children =
        expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Horizontal, "Oscillator");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "freq");
}

#[test]
fn propagate_with_ui_extracts_metadata_after_relative_widget_rebase() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let foo = bb.ident("Foo");
        let faa = bb.ident("Faa");
        let gain = bb.ident("../gain [style:knob]");
        let init = bb.real(0.5);
        let min = bb.real(0.0);
        let max = bb.real(1.0);
        let step = bb.real(0.01);
        let slider = bb.hslider(gain, init, min, max, step);
        let inner = bb.vgroup(faa, slider);
        bb.hgroup(foo, inner)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("relative widget path with metadata should propagate");

    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Horizontal, "Foo");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls[0].label, "gain");
    assert_eq!(
        out.ui.controls[0].metadata,
        vec![("style".to_owned(), "knob".to_owned())]
    );
}

#[test]
fn propagate_with_ui_rebases_explicit_group_label_to_parent() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let foo = bb.ident("Foo");
        let bar = bb.ident("../Bar");
        let gain = bb.ident("gain");
        let init = bb.real(0.5);
        let min = bb.real(0.0);
        let max = bb.real(1.0);
        let step = bb.real(0.01);
        let slider = bb.hslider(gain, init, min, max, step);
        let rebased = bb.vgroup(bar, slider);
        bb.hgroup(foo, rebased)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("relative group label should propagate");

    assert_eq!(out.ui.root_origin, UiRootOrigin::Synthesized);
    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "");
    assert_eq!(root_children.len(), 2);
    assert!(expect_ui_group(&out.ui, root_children[0], UiGroupKind::Horizontal, "Foo").is_empty());
    let bar_children = expect_ui_group(&out.ui, root_children[1], UiGroupKind::Vertical, "Bar");
    assert_eq!(bar_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, bar_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls[0].label, "gain");
}

#[test]
fn propagate_with_ui_clamps_relative_group_label_navigation_at_root() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let foo = bb.ident("Foo");
        let bar = bb.ident("../../../../Bar");
        let gain = bb.ident("gain");
        let init = bb.real(0.5);
        let min = bb.real(0.0);
        let max = bb.real(1.0);
        let step = bb.real(0.01);
        let slider = bb.hslider(gain, init, min, max, step);
        let rebased = bb.vgroup(bar, slider);
        bb.hgroup(foo, rebased)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("clamped relative group label should propagate");

    assert_eq!(out.ui.root_origin, UiRootOrigin::Synthesized);
    let root_children = expect_ui_group(&out.ui, out.ui.root, UiGroupKind::Vertical, "");
    assert_eq!(root_children.len(), 2);
    assert!(expect_ui_group(&out.ui, root_children[0], UiGroupKind::Horizontal, "Foo").is_empty());
    let bar_children = expect_ui_group(&out.ui, root_children[1], UiGroupKind::Vertical, "Bar");
    assert_eq!(bar_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, bar_children[0]),
        UiMatch::InputControl(0)
    );
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

    let flat_soundfile = try_build_flat_box(&arena, soundfile).unwrap();
    let out = propagate_typed(&mut arena, flat_soundfile, &inputs, &mut ArityCache::new())
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

    let flat_ondemand = try_build_flat_box(&arena, ondemand).unwrap();
    let od_zero = propagate_typed(
        &mut arena,
        flat_ondemand,
        &[zero, x],
        &mut ArityCache::new(),
    )
    .expect("ondemand zero clock should propagate");
    assert_eq!(od_zero, vec![zero]);

    let od_one = propagate_typed(&mut arena, flat_ondemand, &[one, x], &mut ArityCache::new())
        .expect("ondemand one clock should bypass wrapper");
    assert_eq!(od_one, vec![x]);

    let od = propagate_typed(&mut arena, flat_ondemand, &[h, x], &mut ArityCache::new())
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

    let flat_upsampling = try_build_flat_box(&arena, upsampling).unwrap();
    let us = propagate_typed(&mut arena, flat_upsampling, &[h, x], &mut ArityCache::new())
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

    let flat_downsampling = try_build_flat_box(&arena, downsampling).unwrap();
    let ds = propagate_typed(
        &mut arena,
        flat_downsampling,
        &[h, x],
        &mut ArityCache::new(),
    )
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

    let flat_route = try_build_flat_box(&arena, route).unwrap();
    let arity = box_arity_typed(&arena, flat_route, &mut ArityCache::new())
        .expect("route arity should infer");
    assert_eq!(arity.inputs, 2);
    assert_eq!(arity.outputs, 2);

    let out = propagate_typed(&mut arena, flat_route, &inputs, &mut ArityCache::new())
        .expect("route should propagate");
    assert_eq!(out.len(), 2);
    let SigMatch::BinOp(BinOp::Add, lhs, rhs) = match_sig(&arena, out[0]) else {
        panic!("first route output should be an add");
    };
    assert_eq!(lhs, inputs[0]);
    assert_eq!(rhs, inputs[1]);
    assert_eq!(match_sig(&arena, out[1]), SigMatch::Int(0));
}

#[test]
fn route_box_ignores_out_of_range_endpoints_like_cpp() {
    let mut arena = TreeArena::new();
    let route = {
        let mut bb = BoxBuilder::new(&mut arena);
        let two = bb.int(2);
        let zero = bb.int(0);
        let one_a = bb.int(1);
        let one_b = bb.int(1);
        let two_a = bb.int(2);
        let two_b = bb.int(2);
        let three_a = bb.int(3);
        let one_c = bb.int(1);
        let p1 = bb.par(zero, one_a);
        let p2 = bb.par(one_b, three_a);
        let p3 = bb.par(two_a, two_b);
        let p4 = bb.par(three_a, one_c);
        let left = bb.par(p1, p2);
        let right = bb.par(p3, p4);
        let spec = bb.par(left, right);
        bb.route(two, two, spec)
    };
    let inputs = make_sig_input_list(&mut arena, 2);

    let flat_route = try_build_flat_box(&arena, route).unwrap();
    let out = propagate_typed(&mut arena, flat_route, &inputs, &mut ArityCache::new())
        .expect("route should propagate");
    assert_eq!(out.len(), 2);
    assert_eq!(match_sig(&arena, out[0]), SigMatch::Int(0));
    assert_eq!(match_sig(&arena, out[1]), SigMatch::Input(1));
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

    let flat_wrapped = try_build_flat_box(&arena, wrapped).unwrap();
    let arity = box_arity_typed(&arena, flat_wrapped, &mut ArityCache::new())
        .expect("ffun arity should infer");
    assert_eq!(arity.inputs, 1);
    assert_eq!(arity.outputs, 1);

    let out = propagate_typed(&mut arena, flat_wrapped, &inputs, &mut ArityCache::new())
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
        let ident = BoxBuilder::new(&mut arena).ident("x");
        let rule_l = BoxBuilder::new(&mut arena).wire();
        let rule_r = BoxBuilder::new(&mut arena).wire();
        let rules = BoxBuilder::new(&mut arena).par(rule_l, rule_r);
        let abstr_body = BoxBuilder::new(&mut arena).wire();
        let modulation_body = BoxBuilder::new(&mut arena).wire();
        let with_lhs = BoxBuilder::new(&mut arena).wire();
        let with_rhs = BoxBuilder::new(&mut arena).wire();
        let modif_lhs = BoxBuilder::new(&mut arena).wire();
        let modif_rhs = BoxBuilder::new(&mut arena).wire();
        let withrec_body = BoxBuilder::new(&mut arena).wire();
        let withrec_name_a = BoxBuilder::new(&mut arena).ident("a");
        let withrec_name_b = BoxBuilder::new(&mut arena).ident("b");
        let withrec_expr_a = BoxBuilder::new(&mut arena).wire();
        let withrec_expr_b = BoxBuilder::new(&mut arena).wire();
        let withrec_defs = {
            let def_a = parser_definition(&mut arena, withrec_name_a, withrec_expr_a);
            let def_b = parser_definition(&mut arena, withrec_name_b, withrec_expr_b);
            let tail = arena.cons(def_b, arena.nil());
            arena.cons(def_a, tail)
        };
        let withrec_defs2 = {
            let def = parser_definition(&mut arena, withrec_name_a, withrec_expr_a);
            arena.cons(def, arena.nil())
        };
        let appl_fun = BoxBuilder::new(&mut arena).wire();
        let appl_arg = BoxBuilder::new(&mut arena).wire();
        let access_expr = BoxBuilder::new(&mut arena).wire();
        let ipar_count = BoxBuilder::new(&mut arena).int(2);
        let ipar_body = BoxBuilder::new(&mut arena).wire();
        let iseq_count = BoxBuilder::new(&mut arena).int(2);
        let iseq_body = BoxBuilder::new(&mut arena).wire();
        let isum_count = BoxBuilder::new(&mut arena).int(2);
        let isum_body = BoxBuilder::new(&mut arena).wire();
        let iprod_count = BoxBuilder::new(&mut arena).int(2);
        let iprod_body = BoxBuilder::new(&mut arena).wire();
        let ff_sig = BoxBuilder::new(&mut arena).wire();
        let ff_inc = BoxBuilder::new(&mut arena).wire();
        let ff_lib = BoxBuilder::new(&mut arena).wire();
        let mut bb = BoxBuilder::new(&mut arena);
        vec![
            (bb.case(rules), "case"),
            (bb.pattern_var(ident), "patternvar"),
            (bb.abstr(ident, abstr_body), "abstr"),
            (bb.modulation(ident, modulation_body), "modulation"),
            (bb.with_local_def(with_lhs, with_rhs), "withlocaldef"),
            (bb.modif_local_def(modif_lhs, modif_rhs), "modiflocaldef"),
            (
                bb.with_rec_def(withrec_body, withrec_defs, withrec_defs2),
                "withlocaldef",
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

    let err = try_build_flat_box(&arena, bad_case)
        .map_err(PropagateError::from)
        .and_then(|f| box_arity_typed(&arena, f, &mut ArityCache::new()))
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
fn propagate_typed_enforces_flat_boundary() {
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
    assert_eq!(typed_with_ui.signals, typed_out);

    let err = try_build_flat_box(&arena, bad_case)
        .map_err(PropagateError::from)
        .and_then(|f| propagate_typed(&mut arena, f, &[], &mut ArityCache::new()))
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

    let flat_pow = try_build_flat_box(&arena, pow).unwrap();
    let pow_out = propagate_typed(&mut arena, flat_pow, &inputs, &mut ArityCache::new())
        .expect("pow should propagate");
    let flat_min = try_build_flat_box(&arena, min).unwrap();
    let min_out = propagate_typed(&mut arena, flat_min, &inputs, &mut ArityCache::new())
        .expect("min should propagate");
    let flat_max = try_build_flat_box(&arena, max).unwrap();
    let max_out = propagate_typed(&mut arena, flat_max, &inputs, &mut ArityCache::new())
        .expect("max should propagate");

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

    let flat_slot7 = try_build_flat_box(&arena, slot7).unwrap();
    let slot_arity = box_arity_typed(&arena, flat_slot7, &mut ArityCache::new())
        .expect("slot arity should infer");
    assert_eq!(slot_arity.inputs, 0);
    assert_eq!(slot_arity.outputs, 1);

    let fallback = propagate_typed(&mut arena, flat_slot7, &[], &mut ArityCache::new())
        .expect("raw slot should lower to deterministic dummy input");
    assert_eq!(match_sig(&arena, fallback[0]), SigMatch::Input(7));

    let inputs = make_sig_input_list(&mut arena, 2);
    let flat_passthrough = try_build_flat_box(&arena, passthrough).unwrap();
    let passthrough_out = propagate_typed(
        &mut arena,
        flat_passthrough,
        &inputs[..1],
        &mut ArityCache::new(),
    )
    .expect("symbolic(slot, slot) should forward its bound input");
    assert_eq!(passthrough_out, vec![inputs[0]]);

    let flat_pair = try_build_flat_box(&arena, pair).unwrap();
    let pair_arity = box_arity_typed(&arena, flat_pair, &mut ArityCache::new())
        .expect("nested symbolic arity should infer");
    assert_eq!(pair_arity.inputs, 2);
    assert_eq!(pair_arity.outputs, 2);

    let pair_out = propagate_typed(&mut arena, flat_pair, &inputs, &mut ArityCache::new())
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

    let [
        flat_acos,
        flat_asin,
        flat_atan,
        flat_atan2,
        flat_cos,
        flat_sin,
        flat_tan,
        flat_exp,
        flat_log,
        flat_log10,
        flat_sqrt,
        flat_abs,
        flat_fmod,
        flat_remainder,
        flat_floor,
        flat_ceil,
        flat_rint,
        flat_round,
    ] = [
        acos, asin, atan, atan2, cos, sin, tan, exp, log, log10, sqrt, abs, fmod, remainder, floor,
        ceil, rint, round,
    ]
    .map(|id| try_build_flat_box(&arena, id).unwrap());

    let acos_sig = propagate_typed(&mut arena, flat_acos, &uinputs, &mut ArityCache::new())
        .expect("acos should propagate")[0];
    let asin_sig = propagate_typed(&mut arena, flat_asin, &uinputs, &mut ArityCache::new())
        .expect("asin should propagate")[0];
    let atan_sig = propagate_typed(&mut arena, flat_atan, &uinputs, &mut ArityCache::new())
        .expect("atan should propagate")[0];
    let atan2_sig = propagate_typed(&mut arena, flat_atan2, &binputs, &mut ArityCache::new())
        .expect("atan2 should propagate")[0];
    let cos_sig = propagate_typed(&mut arena, flat_cos, &uinputs, &mut ArityCache::new())
        .expect("cos should propagate")[0];
    let sin_sig = propagate_typed(&mut arena, flat_sin, &uinputs, &mut ArityCache::new())
        .expect("sin should propagate")[0];
    let tan_sig = propagate_typed(&mut arena, flat_tan, &uinputs, &mut ArityCache::new())
        .expect("tan should propagate")[0];
    let exp_sig = propagate_typed(&mut arena, flat_exp, &uinputs, &mut ArityCache::new())
        .expect("exp should propagate")[0];
    let log_sig = propagate_typed(&mut arena, flat_log, &uinputs, &mut ArityCache::new())
        .expect("log should propagate")[0];
    let log10_sig = propagate_typed(&mut arena, flat_log10, &uinputs, &mut ArityCache::new())
        .expect("log10 should propagate")[0];
    let sqrt_sig = propagate_typed(&mut arena, flat_sqrt, &uinputs, &mut ArityCache::new())
        .expect("sqrt should propagate")[0];
    let abs_sig = propagate_typed(&mut arena, flat_abs, &uinputs, &mut ArityCache::new())
        .expect("abs should propagate")[0];
    let fmod_sig = propagate_typed(&mut arena, flat_fmod, &binputs, &mut ArityCache::new())
        .expect("fmod should propagate")[0];
    let remainder_sig =
        propagate_typed(&mut arena, flat_remainder, &binputs, &mut ArityCache::new())
            .expect("remainder should propagate")[0];
    let floor_sig = propagate_typed(&mut arena, flat_floor, &uinputs, &mut ArityCache::new())
        .expect("floor should propagate")[0];
    let ceil_sig = propagate_typed(&mut arena, flat_ceil, &uinputs, &mut ArityCache::new())
        .expect("ceil should propagate")[0];
    let rint_sig = propagate_typed(&mut arena, flat_rint, &uinputs, &mut ArityCache::new())
        .expect("rint should propagate")[0];
    let round_sig = propagate_typed(&mut arena, flat_round, &uinputs, &mut ArityCache::new())
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
fn flat_box_builder_accepts_autodiff_wrappers() {
    let mut arena = TreeArena::new();
    let (forward, reverse) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let sin = bb.sin();
        (bb.forward_ad(sin), bb.reverse_ad(sin))
    };

    assert!(try_build_flat_box(&arena, forward).is_ok());
    assert!(try_build_flat_box(&arena, reverse).is_ok());
}

#[test]
fn box_arity_typed_expands_forward_ad_outputs() {
    let mut arena = TreeArena::new();
    let (process, wrapped) = {
        let mut bb = BoxBuilder::new(&mut arena);
        let label = bb.ident("freq");
        let init = bb.real(440.0);
        let min = bb.real(50.0);
        let max = bb.real(2_000.0);
        let step = bb.real(1.0);
        let slider = bb.hslider(label, init, min, max, step);
        let sin = bb.sin();
        let process = bb.seq(slider, sin);
        (process, bb.forward_ad(process))
    };

    let inner_arity = box_arity_typed(
        &arena,
        try_build_flat_box(&arena, process).unwrap(),
        &mut ArityCache::new(),
    )
    .expect("inner process arity should infer");
    let wrapped_arity = box_arity_typed(
        &arena,
        try_build_flat_box(&arena, wrapped).unwrap(),
        &mut ArityCache::new(),
    )
    .expect("forward-ad wrapper arity should infer");

    // Inner: hslider : sin → (0, 1)
    assert_eq!(inner_arity.inputs, 0);
    assert_eq!(inner_arity.outputs, 1);
    // fad(inner) expands outputs: 1 * (1 + 1 control) = 2
    assert_eq!(wrapped_arity.inputs, 0);
    assert_eq!(wrapped_arity.outputs, 2);
}

#[test]
fn propagate_forward_ad_expands_outputs_for_single_control() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let label = bb.ident("freq");
        let init = bb.real(440.0);
        let min = bb.real(50.0);
        let max = bb.real(2_000.0);
        let step = bb.real(1.0);
        let slider = bb.hslider(label, init, min, max, step);
        let sin = bb.sin();
        let body = bb.seq(slider, sin);
        bb.forward_ad(body)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("forward-ad process should propagate");

    assert_eq!(out.signals.len(), 2);
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "freq");
    let SigMatch::Sin(primal_input) = match_sig(&arena, out.signals[0]) else {
        panic!("first output should be the primal sin signal");
    };
    assert!(matches!(
        match_sig(&arena, primal_input),
        SigMatch::HSlider(0)
    ));

    let SigMatch::BinOp(BinOp::Mul, lhs, rhs) = match_sig(&arena, out.signals[1]) else {
        panic!("second output should be the tangent mul(cos(x), 1)");
    };
    assert!(matches!(match_sig(&arena, lhs), SigMatch::Cos(_)));
    assert_eq!(match_sig(&arena, rhs), SigMatch::Real(1.0));
}

#[test]
fn propagate_forward_ad_emits_one_tangent_per_enabled_control() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let f_label = bb.ident("f");
        let g_label = bb.ident("g");
        let init = bb.real(1.0);
        let min = bb.real(0.0);
        let max = bb.real(10.0);
        let step = bb.real(0.1);
        let f = bb.hslider(f_label, init, min, max, step);
        let g = bb.hslider(g_label, init, min, max, step);
        let pair = bb.par(f, g);
        let mul = bb.mul();
        let body = bb.seq(pair, mul);
        bb.forward_ad(body)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("forward-ad product should propagate");

    assert_eq!(out.signals.len(), 3);
    assert_eq!(out.ui.controls.len(), 2);
    assert_eq!(out.ui.controls[0].label, "f");
    assert_eq!(out.ui.controls[1].label, "g");
}

#[test]
fn propagate_forward_ad_skips_controls_marked_autodiff_false() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let f_label = bb.ident("f [autodiff:false]");
        let g_label = bb.ident("g");
        let init = bb.real(1.0);
        let min = bb.real(0.0);
        let max = bb.real(10.0);
        let step = bb.real(0.1);
        let f = bb.hslider(f_label, init, min, max, step);
        let g = bb.hslider(g_label, init, min, max, step);
        let pair = bb.par(f, g);
        let mul = bb.mul();
        let body = bb.seq(pair, mul);
        bb.forward_ad(body)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let out = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect("forward-ad product should propagate with autodiff metadata");

    assert_eq!(out.signals.len(), 2);
    assert_eq!(out.ui.controls.len(), 2);
    assert_eq!(
        out.ui.controls[0].metadata,
        vec![("autodiff".to_owned(), "false".to_owned())]
    );
}

#[test]
fn propagate_reverse_ad_returns_clear_unsupported_error() {
    let mut arena = TreeArena::new();
    let process = {
        let mut bb = BoxBuilder::new(&mut arena);
        let label = bb.ident("freq");
        let init = bb.real(440.0);
        let min = bb.real(50.0);
        let max = bb.real(2_000.0);
        let step = bb.real(1.0);
        let slider = bb.hslider(label, init, min, max, step);
        let sin = bb.sin();
        let body = bb.seq(slider, sin);
        bb.reverse_ad(body)
    };

    let flat_process = try_build_flat_box(&arena, process).unwrap();
    let err = propagate_typed_with_ui(&mut arena, flat_process, &[], &mut ArityCache::new())
        .expect_err("reverse-ad should remain unsupported during propagation");

    assert!(matches!(
        err,
        PropagateError::UnsupportedBox {
            kind: "reversead",
            ..
        }
    ));
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
    matches!(tag_name(arena, id), Some(DEBRUIJNREC_TAG))
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
