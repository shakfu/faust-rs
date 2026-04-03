use signals::{BinOp, SigBuilder, SigMatch, match_sig};
use tlib::{de_bruijn_rec, de_bruijn_ref, match_sym_rec, match_sym_ref};
use ui::{ControlKind, ControlSpec};

use super::{
    SignalPrepareError, SimpleSigType, prepare_signals_for_fir, prepare_signals_for_fir_verified,
};

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
fn prepare_signals_for_fir_records_reduced_numeric_types() {
    let mut arena = tlib::TreeArena::new();
    let outputs = {
        let mut b = SigBuilder::new(&mut arena);
        let v0 = b.int(1);
        let v1 = b.int(2);
        let v2 = b.int(3);
        let waveform = b.waveform(&[v0, v1, v2]);
        let input = b.input(0);
        let read = b.rdtbl(waveform, input);
        let selector = b.int(1);
        let zero = b.real(0.0);
        let mix = b.select2(selector, read, zero);
        vec![waveform, read, mix]
    };

    let prepared = prepare_signals_for_fir(&arena, &outputs, &ui::UiProgram::empty())
        .expect("simple numeric typing should work");

    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
    assert_eq!(prepared.ty(prepared.outputs[1]), Some(SimpleSigType::Int));
    assert_eq!(prepared.ty(prepared.outputs[2]), Some(SimpleSigType::Real));
}

#[test]
fn prepare_signals_for_fir_closes_unresolved_recursive_types_to_real() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, self_ref)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("recursive typing should converge");

    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
}

#[test]
fn prepare_signals_for_fir_keeps_integer_recursive_min_feedback_int() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(0, self_ref);
        let prev = b.delay1(feedback);
        let inc = b.int(1);
        let sum = b.add(prev, inc);
        let cap = b.int(3);
        b.min(sum, cap)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("recursive int min should prepare");

    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
    let SigMatch::Proj(_, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should stay a projection");
    };
    let (_, prepared_body_list) =
        match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
    let prepared_body = prepared
        .arena
        .hd(prepared_body_list)
        .expect("prepared recursion body head");
    let SigMatch::Min(sum, cap) = match_sig(&prepared.arena, prepared_body) else {
        panic!("prepared body should stay SIGMIN");
    };
    assert_eq!(match_sig(&prepared.arena, cap), SigMatch::Int(3));
    let SigMatch::BinOp(_, prev, inc) = match_sig(&prepared.arena, sum) else {
        panic!("prepared min lhs should stay integer addition");
    };
    assert!(
        !matches!(match_sig(&prepared.arena, prev), SigMatch::FloatCast(_)),
        "integer recursive feedback should not be promoted to float before SIGMIN"
    );
    assert_eq!(match_sig(&prepared.arena, inc), SigMatch::Int(1));
}

#[test]
fn prepare_signals_for_fir_keeps_integer_recursive_abs_feedback_int() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(0, self_ref);
        let prev = b.delay1(feedback);
        let inc = b.int(1);
        let sum = b.add(prev, inc);
        b.abs(sum)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("recursive int abs should prepare");

    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
    let SigMatch::Proj(_, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should stay a projection");
    };
    let (_, prepared_body_list) =
        match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
    let prepared_body = prepared
        .arena
        .hd(prepared_body_list)
        .expect("prepared recursion body head");
    let SigMatch::Abs(sum) = match_sig(&prepared.arena, prepared_body) else {
        panic!("prepared body should stay SIGABS");
    };
    let SigMatch::BinOp(_, prev, inc) = match_sig(&prepared.arena, sum) else {
        panic!("prepared abs operand should stay integer addition");
    };
    assert!(
        !matches!(match_sig(&prepared.arena, prev), SigMatch::FloatCast(_)),
        "integer recursive feedback should not be promoted to float before SIGABS"
    );
    assert_eq!(match_sig(&prepared.arena, inc), SigMatch::Int(1));
}

#[test]
fn prepare_signals_for_fir_promotes_delay_amounts_to_int() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let input = b.input(0);
        let amount = b.real(1.5);
        b.delay(input, amount)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("delay promotion should succeed");

    let SigMatch::Delay(_, amount) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("promoted output should stay SIGDELAY");
    };
    match match_sig(&prepared.arena, amount) {
        SigMatch::IntCast(inner) => {
            assert_eq!(match_sig(&prepared.arena, inner), SigMatch::Real(1.5));
        }
        SigMatch::Int(1) => {}
        _ => panic!("delay amount should stay as IntCast(real(1.5)) or simplify to Int(1)"),
    }
}

#[test]
fn prepare_signals_for_fir_promotes_select2_selector_and_mixed_branches() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let selector = b.input(0);
        let then_value = b.int(1);
        let else_value = b.input(1);
        b.select2(selector, then_value, else_value)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("select2 promotion should succeed");

    let SigMatch::Select2(selector, then_value, else_value) =
        match_sig(&prepared.arena, prepared.outputs[0])
    else {
        panic!("promoted output should stay SIGSELECT2");
    };
    let SigMatch::IntCast(selector_inner) = match_sig(&prepared.arena, selector) else {
        panic!("select2 selector should be promoted to SIGINTCAST");
    };
    assert_eq!(
        match_sig(&prepared.arena, selector_inner),
        SigMatch::Input(0)
    );
    assert_eq!(
        match_sig(&prepared.arena, then_value),
        SigMatch::Real(1.0),
        "mixed-typed branch should be promoted to real"
    );
    assert_eq!(match_sig(&prepared.arena, else_value), SigMatch::Input(1));
    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
}

#[test]
fn prepare_signals_for_fir_recovers_shared_select2_selector_from_float_context() {
    let mut arena = tlib::TreeArena::new();
    let (arith_out, select_out) = {
        let mut b = SigBuilder::new(&mut arena);
        let input0 = b.input(0);
        let half = b.real(0.5);
        let cmp = b.lt(input0, half);
        let input1 = b.input(1);
        let arith = b.add(cmp, input1);
        let one = b.int(1);
        let zero = b.int(0);
        let sel = b.select2(cmp, one, zero);
        (arith, sel)
    };

    let prepared =
        prepare_signals_for_fir(&arena, &[arith_out, select_out], &ui::UiProgram::empty())
            .expect("shared comparison should promote in both arithmetic and select2 contexts");

    let SigMatch::Select2(selector, _, _) = match_sig(&prepared.arena, prepared.outputs[1]) else {
        panic!("second output should stay SIGSELECT2");
    };
    assert!(
        matches!(match_sig(&prepared.arena, selector), SigMatch::IntCast(_))
            || matches!(
                match_sig(&prepared.arena, selector),
                SigMatch::BinOp(
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                    _,
                    _
                )
            ),
        "shared select2 selector should stay in an integer comparison domain"
    );
}

#[test]
fn prepare_signals_for_fir_recovers_shared_delay_amount_from_float_context() {
    let mut arena = tlib::TreeArena::new();
    let (arith_out, delay_out) = {
        let mut b = SigBuilder::new(&mut arena);
        let input0 = b.input(0);
        let half = b.real(0.5);
        let cmp = b.lt(input0, half);
        let input1 = b.input(1);
        let arith = b.add(cmp, input1);
        let input2 = b.input(2);
        let delay = b.delay(input2, cmp);
        (arith, delay)
    };

    let prepared =
        prepare_signals_for_fir(&arena, &[arith_out, delay_out], &ui::UiProgram::empty())
            .expect("shared comparison should promote in both arithmetic and delay contexts");

    let SigMatch::Delay(_, amount) = match_sig(&prepared.arena, prepared.outputs[1]) else {
        panic!("second output should stay SIGDELAY");
    };
    assert!(
        matches!(match_sig(&prepared.arena, amount), SigMatch::IntCast(_))
            || matches!(
                match_sig(&prepared.arena, amount),
                SigMatch::BinOp(
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                    _,
                    _
                )
            ),
        "shared delay amount should stay in an integer comparison domain"
    );
}

#[test]
fn prepare_signals_for_fir_recovers_shared_rdtbl_index_from_float_context() {
    let mut arena = tlib::TreeArena::new();
    let (arith_out, table_out) = {
        let mut b = SigBuilder::new(&mut arena);
        let input0 = b.input(0);
        let half = b.real(0.5);
        let cmp = b.lt(input0, half);
        let input1 = b.input(1);
        let arith = b.add(cmp, input1);
        let v0 = b.real(0.0);
        let v1 = b.real(1.0);
        let waveform = b.waveform(&[v0, v1]);
        let table = b.rdtbl(waveform, cmp);
        (arith, table)
    };

    let prepared =
        prepare_signals_for_fir(&arena, &[arith_out, table_out], &ui::UiProgram::empty())
            .expect("shared comparison should promote in both arithmetic and rdtbl contexts");

    let SigMatch::RdTbl(_, index) = match_sig(&prepared.arena, prepared.outputs[1]) else {
        panic!("second output should stay SIGRDTBL");
    };
    assert!(
        matches!(match_sig(&prepared.arena, index), SigMatch::IntCast(_))
            || matches!(
                match_sig(&prepared.arena, index),
                SigMatch::BinOp(
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                    _,
                    _
                )
            ),
        "shared table-read index should stay in an integer comparison domain"
    );
}

#[test]
fn prepare_signals_for_fir_recovers_shared_wrtbl_write_signal_from_float_context() {
    let mut arena = tlib::TreeArena::new();
    let (arith_out, table_out) = {
        let mut b = SigBuilder::new(&mut arena);
        let input0 = b.input(0);
        let half = b.real(0.5);
        let cmp = b.lt(input0, half);
        let input1 = b.input(1);
        let arith = b.add(cmp, input1);
        let size = b.int(8);
        let generator = b.int(0);
        let write_index = b.int(1);
        let table = b.wrtbl(size, generator, write_index, cmp);
        (arith, table)
    };

    let prepared =
        prepare_signals_for_fir(&arena, &[arith_out, table_out], &ui::UiProgram::empty())
            .expect("shared comparison should promote in both arithmetic and wrtbl contexts");

    let SigMatch::WrTbl(_, _, _, write_signal) = match_sig(&prepared.arena, prepared.outputs[1])
    else {
        panic!("second output should stay SIGWRTBL");
    };
    assert!(
        matches!(
            match_sig(&prepared.arena, write_signal),
            SigMatch::IntCast(_)
        ) || matches!(
            match_sig(&prepared.arena, write_signal),
            SigMatch::BinOp(
                BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                _,
                _
            )
        ),
        "shared wrtbl write signal should stay in an integer comparison domain"
    );
}

#[test]
fn prepare_signals_for_fir_recovers_shared_zero_pad_amount_from_float_context() {
    let mut arena = tlib::TreeArena::new();
    let (arith_out, padded_out) = {
        let mut b = SigBuilder::new(&mut arena);
        let input0 = b.input(0);
        let half = b.real(0.5);
        let cmp = b.lt(input0, half);
        let input1 = b.input(1);
        let arith = b.add(cmp, input1);
        let input2 = b.input(2);
        let padded = b.zero_pad(input2, cmp);
        (arith, padded)
    };

    let prepared =
        prepare_signals_for_fir(&arena, &[arith_out, padded_out], &ui::UiProgram::empty())
            .expect("shared comparison should promote in both arithmetic and zero_pad contexts");

    let SigMatch::ZeroPad(_, amount) = match_sig(&prepared.arena, prepared.outputs[1]) else {
        panic!("second output should stay SIGZEROPAD");
    };
    assert!(
        matches!(match_sig(&prepared.arena, amount), SigMatch::IntCast(_))
            || matches!(
                match_sig(&prepared.arena, amount),
                SigMatch::BinOp(
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                    _,
                    _
                )
            ),
        "shared zero-pad amount should stay in an integer comparison domain"
    );
}

#[test]
fn prepare_signals_for_fir_promotes_table_read_index_to_int() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let v0 = b.real(0.0);
        let v1 = b.real(1.0);
        let waveform = b.waveform(&[v0, v1]);
        let index = b.input(0);
        b.rdtbl(waveform, index)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("table promotion should succeed");

    let SigMatch::RdTbl(_, index) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("promoted output should stay SIGRDTBL");
    };
    let SigMatch::IntCast(inner) = match_sig(&prepared.arena, index) else {
        panic!("table read index should be promoted to SIGINTCAST");
    };
    assert_eq!(match_sig(&prepared.arena, inner), SigMatch::Input(0));
}

#[test]
fn prepare_signals_for_fir_promotes_real_mul_operands_before_binop() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        let gate_init = b.int(0);
        let gate_next = b.int(1);
        let gate = b.prefix(gate_init, gate_next);
        let carrier_init = b.real(0.0);
        let carrier_next = b.real(0.5);
        let carrier = b.prefix(carrier_init, carrier_next);
        let inner = b.binop(BinOp::Mul, carrier, gate);
        b.binop(BinOp::Mul, inner, gate)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("mixed real/int multiplication should prepare");

    let SigMatch::BinOp(BinOp::Mul, left, right) = match_sig(&prepared.arena, prepared.outputs[0])
    else {
        panic!("prepared output should stay SIGBINOP(Mul, ...)");
    };
    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
    assert_eq!(prepared.ty(left), Some(SimpleSigType::Real));
    assert!(
        matches!(
            prepared.ty(right),
            Some(SimpleSigType::Int) | Some(SimpleSigType::Real)
        ),
        "outer multiplication rhs should stay typed after simplification"
    );

    assert!(
        prepared.ty(left).is_some(),
        "simplified lhs subtree should stay typed for FIR lowering"
    );
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
fn recursive_fixpoint_recomputes_body_types_after_real_widening() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let counter_body = {
        let mut b = SigBuilder::new(&mut arena);
        let counter_proj = b.proj(0, self_ref);
        let counter_prev = b.delay1(counter_proj);
        let one = b.int(1);
        b.binop(BinOp::Add, counter_prev, one)
    };
    let amp_body = {
        let mut b = SigBuilder::new(&mut arena);
        let amp_proj = b.proj(1, self_ref);
        let amp_prev = b.delay1(amp_proj);
        let half = b.real(0.5);
        b.binop(BinOp::Add, amp_prev, half)
    };
    let gated_body = {
        let mut b = SigBuilder::new(&mut arena);
        let amp_proj = b.proj(1, self_ref);
        let counter_proj = b.proj(0, self_ref);
        let amp_prev = b.delay1(amp_proj);
        let counter_prev = b.delay1(counter_proj);
        let period = b.int(128);
        let zero = b.int(0);
        let one = b.int(1);
        let rem = b.binop(BinOp::Rem, counter_prev, period);
        let eq = b.binop(BinOp::Eq, rem, zero);
        let gate = b.binop(BinOp::Sub, one, eq);
        b.binop(BinOp::Mul, amp_prev, gate)
    };
    let nil = arena.nil();
    let tail2 = arena.cons(gated_body, nil);
    let tail1 = arena.cons(amp_body, tail2);
    let body_list = arena.cons(counter_body, tail1);
    let group = de_bruijn_rec(&mut arena, body_list);
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(2, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("recursive real widening should converge");

    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
}

#[test]
fn prepare_signals_for_fir_uses_foreign_function_return_type() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let ty_int = arena.int(0);
        let ty_real = arena.int(1);
        let incfile = arena.symbol("<math.h>");
        let libfile = arena.symbol("\"\"");
        let name_f32 = arena.symbol("isnanf");
        let name_f64 = arena.symbol("isnan");
        let name_f80 = arena.symbol("isnanl");
        let name_fx = arena.symbol("isnanfx");
        let nil = arena.nil();
        let names = {
            let tail0 = arena.cons(name_fx, nil);
            let tail1 = arena.cons(name_f80, tail0);
            let tail2 = arena.cons(name_f64, tail1);
            arena.cons(name_f32, tail2)
        };
        let arg_types = arena.cons(ty_real, nil);
        let payload = arena.cons(names, arg_types);
        let signature = arena.cons(ty_int, payload);
        let ff_tag = arena.intern_tag("FFUN");
        let ff = arena.intern(tlib::NodeKind::Tag(ff_tag), &[signature, incfile, libfile]);
        let input0 = {
            let mut b = SigBuilder::new(&mut arena);
            b.input(0)
        };
        let args = arena.cons(input0, nil);
        let mut b = SigBuilder::new(&mut arena);
        b.ffun(ff, args)
    };

    let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("foreign function result type should prepare");

    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
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
    let feedback = match match_sig(&prepared.arena, prepared_body) {
        SigMatch::Delay1(feedback) => feedback,
        SigMatch::Delay(feedback, amount) => {
            assert_eq!(match_sig(&prepared.arena, amount), SigMatch::Int(1));
            feedback
        }
        _ => panic!("prepared body should stay a one-sample delay"),
    };
    let SigMatch::Proj(0, feedback_group) = match_sig(&prepared.arena, feedback) else {
        panic!("feedback edge should canonicalize to proj(0, symref(var))");
    };
    let (var, _) =
        match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
    assert_eq!(match_sym_ref(&prepared.arena, feedback_group), Some(var));
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
fn prepared_signals_verify_rejects_missing_reduced_type_entry() {
    let mut arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.input(0)
    };

    let mut prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("baseline preparation should succeed");
    prepared.types.remove(&prepared.outputs[0]);

    let err = prepared
        .verify(&ui::UiProgram::empty())
        .expect_err("missing reduced type should fail verification");
    let SignalPrepareError::Validation(message) = err else {
        panic!("expected validation error");
    };
    assert!(message.contains("missing reduced type annotation"));
}

#[test]
fn prepared_signals_verify_rejects_out_of_range_recursive_projection() {
    let mut arena = tlib::TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(0, self_ref);
        b.delay1(feedback)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let output = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let mut prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
        .expect("baseline preparation should succeed");
    let SigMatch::Proj(_, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
        panic!("prepared output should stay a projection");
    };
    let old_output = prepared.outputs[0];
    let old_reduced = prepared.ty(old_output).expect("baseline reduced type");
    let old_full = prepared
        .sig_ty(old_output)
        .cloned()
        .expect("baseline full type");
    let bad_output = {
        let mut b = SigBuilder::new(&mut prepared.arena);
        b.proj(3, prepared_group)
    };
    prepared.outputs[0] = bad_output;
    prepared.types.insert(bad_output, old_reduced);
    prepared.sig_types.insert(bad_output, old_full);

    let err = prepared
        .verify(&ui::UiProgram::empty())
        .expect_err("out-of-range recursive projection should fail verification");
    let SignalPrepareError::Validation(message) = err else {
        panic!("expected validation error");
    };
    assert!(message.contains("out of range"));
}

#[test]
fn prepared_signals_verify_rejects_missing_ui_control_reference() {
    let mut signal_arena = tlib::TreeArena::new();
    let output = {
        let mut b = SigBuilder::new(&mut signal_arena);
        b.button(0)
    };

    let mut ui = ui::UiProgram::empty();
    ui.controls.push(ControlSpec {
        id: 0,
        kind: ControlKind::Button,
        label: "button".to_owned(),
        metadata: Vec::new(),
        range: None,
    });

    let prepared = prepare_signals_for_fir(&signal_arena, &[output], &ui)
        .expect("baseline preparation with matching UI should succeed");
    let err = prepared
        .verify(&ui::UiProgram::empty())
        .expect_err("missing UI control registry entry should fail verification");
    let SignalPrepareError::Validation(message) = err else {
        panic!("expected validation error");
    };
    assert!(message.contains("missing UI control id 0"));
}
