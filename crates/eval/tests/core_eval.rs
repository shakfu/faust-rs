use boxes::{BoxBuilder, BoxMatch, match_box};
use eval::{Environment, EvalError, LoopDetector, eval_box, eval_process};
use tlib::TreeArena;

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
