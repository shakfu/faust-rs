use boxes::{BoxBuilder, BoxMatch, match_box};
use propagate::{PropagateError, box_arity, make_sig_input_list, propagate};
use signals::{BinOp, SigBuilder, SigMatch, match_sig};
use tlib::TreeArena;

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
fn propagate_reports_arity_mismatch_and_unsupported_rec() {
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

    let err = propagate(&mut arena, rec, &[]).expect_err("rec not yet implemented");
    assert!(matches!(
        err,
        PropagateError::UnsupportedBox { kind: "rec", .. }
    ));
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
