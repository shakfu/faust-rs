use boxes::{BoxBuilder, BoxMatch, match_box};
use propagate::{PropagateError, box_arity, make_sig_input_list, propagate};
use signals::{BinOp, SigBuilder, SigMatch, match_sig};
use tlib::{NodeKind, TreeArena, TreeId};

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
    let out = propagate(&mut arena, add, &inputs).expect("add should propagate");
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
    let arity_seq = box_arity(&arena, seq).expect("seq arity should infer");
    assert_eq!(arity_seq.inputs, 2);
    assert_eq!(arity_seq.outputs, 1);

    let seq_inputs = make_sig_input_list(&mut arena, 2);
    let seq_out = propagate(&mut arena, seq, &seq_inputs).expect("seq should propagate");
    assert_eq!(
        match_sig(&arena, seq_out[0]),
        SigMatch::BinOp(BinOp::Add, seq_inputs[0], seq_inputs[1])
    );

    let split_inputs = make_sig_input_list(&mut arena, 1);
    let split_out = propagate(&mut arena, split, &split_inputs).expect("split should propagate");
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

    let out = propagate(&mut arena, merge, &inputs).expect("merge should propagate");
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
    let err = propagate(&mut arena, bad_seq, &[sig0]).expect_err("bad seq must fail");
    assert!(matches!(err, PropagateError::SeqArityMismatch { .. }));

    let rec = {
        let mut bb = BoxBuilder::new(&mut arena);
        let wire = bb.wire();
        bb.rec(wire, wire)
    };
    let rec_arity = box_arity(&arena, rec).expect("rec arity should infer");
    assert_eq!(rec_arity.inputs, 0);
    assert_eq!(rec_arity.outputs, 1);

    let rec_out = propagate(&mut arena, rec, &[]).expect("rec should propagate");
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
    let out = propagate(&mut arena, rec, &inputs).expect("rec +~_ should propagate");
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
    let out = propagate(&mut arena, rec, &inputs).expect("mixed rec should propagate");
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

    let iout = propagate(&mut arena, inputs_box, &[]).expect("inputs(...) should propagate");
    let oout = propagate(&mut arena, outputs_box, &[]).expect("outputs(...) should propagate");

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

    let arity = box_arity(&arena, waveform).expect("waveform arity should infer");
    assert_eq!(arity.inputs, 0);
    assert_eq!(arity.outputs, 2);

    let out = propagate(&mut arena, waveform, &[]).expect("waveform should propagate");
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

#[test]
fn propagate_pow_min_max_map_to_signal_nodes() {
    let mut arena = TreeArena::new();
    let (pow, min, max) = {
        let mut bb = BoxBuilder::new(&mut arena);
        (bb.pow(), bb.min(), bb.max())
    };
    let inputs = make_sig_input_list(&mut arena, 2);

    let pow_out = propagate(&mut arena, pow, &inputs).expect("pow should propagate");
    let min_out = propagate(&mut arena, min, &inputs).expect("min should propagate");
    let max_out = propagate(&mut arena, max, &inputs).expect("max should propagate");

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

    let acos_sig = propagate(&mut arena, acos, &uinputs).expect("acos should propagate")[0];
    let asin_sig = propagate(&mut arena, asin, &uinputs).expect("asin should propagate")[0];
    let atan_sig = propagate(&mut arena, atan, &uinputs).expect("atan should propagate")[0];
    let atan2_sig = propagate(&mut arena, atan2, &binputs).expect("atan2 should propagate")[0];
    let cos_sig = propagate(&mut arena, cos, &uinputs).expect("cos should propagate")[0];
    let sin_sig = propagate(&mut arena, sin, &uinputs).expect("sin should propagate")[0];
    let tan_sig = propagate(&mut arena, tan, &uinputs).expect("tan should propagate")[0];
    let exp_sig = propagate(&mut arena, exp, &uinputs).expect("exp should propagate")[0];
    let log_sig = propagate(&mut arena, log, &uinputs).expect("log should propagate")[0];
    let log10_sig = propagate(&mut arena, log10, &uinputs).expect("log10 should propagate")[0];
    let sqrt_sig = propagate(&mut arena, sqrt, &uinputs).expect("sqrt should propagate")[0];
    let abs_sig = propagate(&mut arena, abs, &uinputs).expect("abs should propagate")[0];
    let fmod_sig = propagate(&mut arena, fmod, &binputs).expect("fmod should propagate")[0];
    let remainder_sig =
        propagate(&mut arena, remainder, &binputs).expect("remainder should propagate")[0];
    let floor_sig = propagate(&mut arena, floor, &uinputs).expect("floor should propagate")[0];
    let ceil_sig = propagate(&mut arena, ceil, &uinputs).expect("ceil should propagate")[0];
    let rint_sig = propagate(&mut arena, rint, &uinputs).expect("rint should propagate")[0];
    let round_sig = propagate(&mut arena, round, &uinputs).expect("round should propagate")[0];

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
