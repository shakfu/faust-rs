use boxes::{BoxBuilder, BoxMatch, match_box};
use eval::{Environment, EvalError, LoopDetector, eval_box, eval_process};
use tlib::{TreeArena, TreeId};

fn make_ident(arena: &mut TreeArena, name: &str) -> tlib::TreeId {
    BoxBuilder::new(arena).ident(name)
}

fn make_wire(arena: &mut TreeArena) -> tlib::TreeId {
    BoxBuilder::new(arena).wire()
}

fn make_def(
    arena: &mut TreeArena,
    name: &str,
    args: tlib::TreeId,
    expr: tlib::TreeId,
) -> tlib::TreeId {
    let ident = make_ident(arena, name);
    let payload = arena.cons(args, expr);
    arena.cons(ident, payload)
}

fn make_defs(arena: &mut TreeArena, defs: &[tlib::TreeId]) -> tlib::TreeId {
    let mut out = arena.nil();
    for def in defs.iter().rev() {
        out = arena.cons(*def, out);
    }
    out
}

fn make_rev_list2(arena: &mut TreeArena, a: TreeId, b: TreeId) -> TreeId {
    let nil = arena.nil();
    let t = arena.cons(a, nil);
    arena.cons(b, t)
}

fn expect_int(arena: &TreeArena, id: TreeId, expected: i64) {
    assert_eq!(match_box(arena, id), BoxMatch::Int(expected));
}

fn count_add_nodes(arena: &TreeArena, root: TreeId) -> usize {
    let mut n = 0usize;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if matches!(match_box(arena, id), BoxMatch::Add) {
            n += 1;
        }
        if let Some(node) = arena.node(id) {
            for child in node.children.as_slice() {
                stack.push(*child);
            }
        }
    }
    n
}

#[test]
fn eval_process_resolves_named_definition() {
    let mut arena = TreeArena::new();
    let wire = make_wire(&mut arena);
    let nil = arena.nil();
    let foo = make_def(&mut arena, "foo", nil, wire);
    let foo_ident = make_ident(&mut arena, "foo");
    let process = make_def(&mut arena, "process", nil, foo_ident);
    let root = make_defs(&mut arena, &[foo, process]);

    let out = eval_process(&mut arena, root).expect("evaluation should succeed");
    assert!(matches!(match_box(&arena, out), BoxMatch::Wire));
}

#[test]
fn eval_box_resolves_with_local_scope() {
    let mut arena = TreeArena::new();
    let wire = make_wire(&mut arena);
    let nil = arena.nil();
    let local_def = make_def(&mut arena, "a", nil, wire);
    let locals = make_defs(&mut arena, &[local_def]);
    let a_ident = make_ident(&mut arena, "a");
    let expr = BoxBuilder::new(&mut arena).with_local_def(a_ident, locals);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("with local should resolve");
    assert!(matches!(match_box(&arena, out), BoxMatch::Wire));
}

#[test]
fn eval_process_reports_missing_process() {
    let mut arena = TreeArena::new();
    let nil = arena.nil();
    let foo_wire = make_wire(&mut arena);
    let foo = make_def(&mut arena, "foo", nil, foo_wire);
    let root = make_defs(&mut arena, &[foo]);
    let err = eval_process(&mut arena, root).expect_err("missing process should fail");
    assert_eq!(err, EvalError::MissingProcessDefinition);
}

#[test]
fn eval_process_detects_recursive_loop() {
    let mut arena = TreeArena::new();
    let nil = arena.nil();
    let process_ident = make_ident(&mut arena, "process");
    let foo = make_def(&mut arena, "foo", nil, process_ident);
    let foo_ident = make_ident(&mut arena, "foo");
    let process = make_def(&mut arena, "process", nil, foo_ident);
    let root = make_defs(&mut arena, &[foo, process]);

    let err = eval_process(&mut arena, root).expect_err("recursive cycle should fail");
    assert!(matches!(err, EvalError::LoopDetected { .. }));
}

#[test]
fn eval_process_applies_function_arguments_in_cpp_order() {
    let mut arena = TreeArena::new();
    let nil = arena.nil();
    let x = make_ident(&mut arena, "x");
    let y = make_ident(&mut arena, "y");
    let params_rev = make_rev_list2(&mut arena, x, y); // parser-style reverse list: [y, x]
    let body = {
        let par = BoxBuilder::new(&mut arena).par(x, y);
        let add = BoxBuilder::new(&mut arena).add();
        BoxBuilder::new(&mut arena).seq(par, add)
    };
    let foo = make_def(&mut arena, "foo", params_rev, body);

    let one = BoxBuilder::new(&mut arena).int(1);
    let two = BoxBuilder::new(&mut arena).int(2);
    let args_rev = make_rev_list2(&mut arena, one, two); // parser-style reverse list: [2, 1]
    let process_expr = {
        let foo_ident = make_ident(&mut arena, "foo");
        BoxBuilder::new(&mut arena).appl(foo_ident, args_rev)
    };
    let process = make_def(&mut arena, "process", nil, process_expr);
    let root = make_defs(&mut arena, &[foo, process]);

    let out = eval_process(&mut arena, root).expect("application should evaluate");
    let (lhs, rhs) = match match_box(&arena, out) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected BOXSEQ, got {other:?}"),
    };
    assert!(matches!(match_box(&arena, rhs), BoxMatch::Add));
    let (a, b) = match match_box(&arena, lhs) {
        BoxMatch::Par(a, b) => (a, b),
        other => panic!("expected BOXPAR, got {other:?}"),
    };
    expect_int(&arena, a, 1);
    expect_int(&arena, b, 2);
}

#[test]
fn eval_box_non_closure_application_falls_back_to_seq_par() {
    let mut arena = TreeArena::new();
    let one = BoxBuilder::new(&mut arena).int(1);
    let two = BoxBuilder::new(&mut arena).int(2);
    let args_rev = make_rev_list2(&mut arena, one, two); // [2,1]
    let add = BoxBuilder::new(&mut arena).add();
    let expr = BoxBuilder::new(&mut arena).appl(add, args_rev);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("non-closure apply should lower to seq(par(args), fun)");
    let (lhs, rhs) = match match_box(&arena, out) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected BOXSEQ, got {other:?}"),
    };
    assert!(matches!(match_box(&arena, rhs), BoxMatch::Add));
    let (a, b) = match match_box(&arena, lhs) {
        BoxMatch::Par(a, b) => (a, b),
        other => panic!("expected BOXPAR, got {other:?}"),
    };
    expect_int(&arena, a, 1);
    expect_int(&arena, b, 2);
}

#[test]
fn eval_iterative_par_expands_with_index_binding() {
    let mut arena = TreeArena::new();
    let i = make_ident(&mut arena, "i");
    let three = BoxBuilder::new(&mut arena).int(3);
    let expr = BoxBuilder::new(&mut arena).ipar(i, three, i);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("ipar should expand");
    let (a0, r1) = match match_box(&arena, out) {
        BoxMatch::Par(a0, r1) => (a0, r1),
        other => panic!("expected top BOXPAR, got {other:?}"),
    };
    expect_int(&arena, a0, 0);
    let (a1, a2) = match match_box(&arena, r1) {
        BoxMatch::Par(a1, a2) => (a1, a2),
        other => panic!("expected second BOXPAR, got {other:?}"),
    };
    expect_int(&arena, a1, 1);
    expect_int(&arena, a2, 2);
}

#[test]
fn eval_iterative_sum_builds_add_chain() {
    let mut arena = TreeArena::new();
    let i = make_ident(&mut arena, "i");
    let three = BoxBuilder::new(&mut arena).int(3);
    let expr = BoxBuilder::new(&mut arena).isum(i, three, i);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("isum should expand");
    assert_eq!(count_add_nodes(&arena, out), 2);
}
