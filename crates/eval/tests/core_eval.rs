use boxes::{BoxBuilder, BoxMatch, match_box};
use errors::{IntoDiagnostic, Severity, Stage, codes};
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

fn make_rev_list3(arena: &mut TreeArena, a: TreeId, b: TreeId, c: TreeId) -> TreeId {
    let nil = arena.nil();
    let t1 = arena.cons(a, nil);
    let t2 = arena.cons(b, t1);
    arena.cons(c, t2)
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

fn make_rule1(arena: &mut TreeArena, pattern: TreeId, rhs: TreeId) -> TreeId {
    let nil = arena.nil();
    let lhs = arena.cons(pattern, nil);
    arena.cons(lhs, rhs)
}

fn make_rules_parser_order(arena: &mut TreeArena, source_rules: &[TreeId]) -> TreeId {
    let mut out = arena.nil();
    for rule in source_rules {
        out = arena.cons(*rule, out);
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
fn eval_box_non_closure_partial_binary_primitive_prepends_missing_wire() {
    let mut arena = TreeArena::new();
    let half = BoxBuilder::new(&mut arena).real(0.5);
    let nil = arena.nil();
    let args = arena.cons(half, nil);
    let mul = BoxBuilder::new(&mut arena).mul();
    let expr = BoxBuilder::new(&mut arena).appl(mul, args);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("partial binary primitive should insert missing wire");
    let (lhs, rhs) = match match_box(&arena, out) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected BOXSEQ, got {other:?}"),
    };
    assert!(matches!(match_box(&arena, rhs), BoxMatch::Mul));
    let (a, b) = match match_box(&arena, lhs) {
        BoxMatch::Par(a, b) => (a, b),
        other => panic!("expected BOXPAR, got {other:?}"),
    };
    assert!(matches!(match_box(&arena, a), BoxMatch::Wire));
    assert!(matches!(match_box(&arena, b), BoxMatch::Real(v) if (v - 0.5).abs() < f64::EPSILON));
}

#[test]
fn eval_box_non_closure_partial_prefix_appends_missing_wire() {
    let mut arena = TreeArena::new();
    let zero = BoxBuilder::new(&mut arena).int(0);
    let nil = arena.nil();
    let args = arena.cons(zero, nil);
    let prefix = BoxBuilder::new(&mut arena).prefix();
    let expr = BoxBuilder::new(&mut arena).appl(prefix, args);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("partial prefix should insert missing wire");
    let (lhs, rhs) = match match_box(&arena, out) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected BOXSEQ, got {other:?}"),
    };
    assert!(matches!(match_box(&arena, rhs), BoxMatch::Prefix));
    let (a, b) = match match_box(&arena, lhs) {
        BoxMatch::Par(a, b) => (a, b),
        other => panic!("expected BOXPAR, got {other:?}"),
    };
    expect_int(&arena, a, 0);
    assert!(matches!(match_box(&arena, b), BoxMatch::Wire));
}

#[test]
fn eval_box_non_closure_application_reports_too_many_arguments() {
    let mut arena = TreeArena::new();
    let one = BoxBuilder::new(&mut arena).int(1);
    let two = BoxBuilder::new(&mut arena).int(2);
    let three = BoxBuilder::new(&mut arena).int(3);
    let args_rev = make_rev_list3(&mut arena, one, two, three); // [3,2,1]
    let add = BoxBuilder::new(&mut arena).add();
    let expr = BoxBuilder::new(&mut arena).appl(add, args_rev);

    let mut loop_detector = LoopDetector::new();
    let err = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect_err("add with 3 arguments should fail");
    assert_eq!(
        err,
        EvalError::TooManyArguments {
            expected: 2,
            got: 3
        }
    );
}

#[test]
fn eval_box_access_reads_environment_local_binding() {
    let mut arena = TreeArena::new();
    let nil = arena.nil();

    let env_box = BoxBuilder::new(&mut arena).environment();
    let wire = make_wire(&mut arena);
    let a_def = make_def(&mut arena, "a", nil, wire);
    let defs = make_defs(&mut arena, &[a_def]);
    let env_with_defs = BoxBuilder::new(&mut arena).with_local_def(env_box, defs);
    let field = make_ident(&mut arena, "a");
    let expr = BoxBuilder::new(&mut arena).access(env_with_defs, field);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("access should resolve from local environment");
    assert!(matches!(match_box(&arena, out), BoxMatch::Wire));
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

#[test]
fn eval_case_uses_source_rule_priority() {
    let mut arena = TreeArena::new();
    let p0 = BoxBuilder::new(&mut arena).int(0);
    let r1 = BoxBuilder::new(&mut arena).int(1);
    let x_ident = make_ident(&mut arena, "x");
    let px = BoxBuilder::new(&mut arena).pattern_var(x_ident);
    let r2 = BoxBuilder::new(&mut arena).int(2);
    let rule1 = make_rule1(&mut arena, p0, r1); // source first
    let rule2 = make_rule1(&mut arena, px, r2); // source second
    let rules = make_rules_parser_order(&mut arena, &[rule1, rule2]);
    let case_expr = BoxBuilder::new(&mut arena).case(rules);
    let arg0 = BoxBuilder::new(&mut arena).int(0);
    let nil = arena.nil();
    let args = arena.cons(arg0, nil);
    let expr = BoxBuilder::new(&mut arena).appl(case_expr, args);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("case application should match first source rule");
    expect_int(&arena, out, 1);
}

#[test]
fn eval_case_pattern_var_binds_argument() {
    let mut arena = TreeArena::new();
    let x_ident = make_ident(&mut arena, "x");
    let px = BoxBuilder::new(&mut arena).pattern_var(x_ident);
    let rhs = make_ident(&mut arena, "x");
    let rule = make_rule1(&mut arena, px, rhs);
    let rules = make_rules_parser_order(&mut arena, &[rule]);
    let case_expr = BoxBuilder::new(&mut arena).case(rules);
    let arg = BoxBuilder::new(&mut arena).int(7);
    let nil = arena.nil();
    let args = arena.cons(arg, nil);
    let expr = BoxBuilder::new(&mut arena).appl(case_expr, args);

    let mut loop_detector = LoopDetector::new();
    let out = eval_box(&mut arena, expr, &Environment::empty(), &mut loop_detector)
        .expect("pattern var should bind");
    expect_int(&arena, out, 7);
}

#[test]
fn eval_case_reports_arity_mismatch_and_no_match() {
    let mut arena = TreeArena::new();
    let x_ident = make_ident(&mut arena, "x");
    let y_ident = make_ident(&mut arena, "y");
    let x = BoxBuilder::new(&mut arena).pattern_var(x_ident);
    let y = BoxBuilder::new(&mut arena).pattern_var(y_ident);
    let lhs_rev = make_rev_list2(&mut arena, x, y); // parser-style reverse list for (x, y)
    let rhs = BoxBuilder::new(&mut arena).int(99);
    let rule = arena.cons(lhs_rev, rhs);
    let rules = make_rules_parser_order(&mut arena, &[rule]);
    let case_expr = BoxBuilder::new(&mut arena).case(rules);
    let one = BoxBuilder::new(&mut arena).int(1);
    let nil = arena.nil();
    let args_one = arena.cons(one, nil);
    let expr_arity = BoxBuilder::new(&mut arena).appl(case_expr, args_one);

    let mut loop_detector = LoopDetector::new();
    let err = eval_box(
        &mut arena,
        expr_arity,
        &Environment::empty(),
        &mut loop_detector,
    )
    .expect_err("arity mismatch should fail");
    assert!(matches!(
        err,
        EvalError::PatternArityMismatch {
            expected: 2,
            got: 1
        }
    ));

    // No-match branch: (0) => 1 applied to 2.
    let p0 = BoxBuilder::new(&mut arena).int(0);
    let r1 = BoxBuilder::new(&mut arena).int(1);
    let one_rule = make_rule1(&mut arena, p0, r1);
    let one_rules = make_rules_parser_order(&mut arena, &[one_rule]);
    let case_no_match = BoxBuilder::new(&mut arena).case(one_rules);
    let two = BoxBuilder::new(&mut arena).int(2);
    let nil2 = arena.nil();
    let args_two = arena.cons(two, nil2);
    let expr_no_match = BoxBuilder::new(&mut arena).appl(case_no_match, args_two);

    let mut loop_detector2 = LoopDetector::new();
    let err2 = eval_box(
        &mut arena,
        expr_no_match,
        &Environment::empty(),
        &mut loop_detector2,
    )
    .expect_err("no matching rule should fail");
    assert!(matches!(err2, EvalError::PatternMatchFailed));
}

#[test]
fn eval_error_converts_to_structured_diagnostic_codes() {
    let missing = EvalError::MissingProcessDefinition.into_diagnostic();
    assert_eq!(missing.severity, Severity::Error);
    assert_eq!(missing.stage, Stage::Eval);
    assert_eq!(missing.code, codes::EVAL_MISSING_PROCESS);
    assert!(!missing.help.is_empty());

    let undef = EvalError::UndefinedSymbol {
        symbol: "foo".to_owned(),
    }
    .into_diagnostic();
    assert_eq!(undef.code, codes::EVAL_UNDEFINED_SYMBOL);

    let iter = EvalError::NegativeIterationCount { value: -1 }.into_diagnostic();
    assert_eq!(iter.code, codes::EVAL_ITERATION_INVALID);
    assert!(!iter.help.is_empty());

    let arity = EvalError::TooManyArguments {
        expected: 2,
        got: 3,
    }
    .into_diagnostic();
    assert_eq!(arity.code, codes::EVAL_ARITY_MISMATCH);
    assert!(!arity.notes.is_empty());
}
