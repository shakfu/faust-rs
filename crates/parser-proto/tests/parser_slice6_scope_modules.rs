use boxes::{
    dump_box, is_box_component, is_box_environment, is_box_library, is_box_real, is_box_route,
    is_box_waveform, is_box_with_local_def, is_box_with_rec_def,
};
use parser_proto::parse_program;
use tlib::{NodeKind, TreeArena, TreeId};

fn list_head(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.hd(list).expect("list must be non-empty")
}

fn list_tail(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.tl(list).expect("list tail must exist")
}

fn definition_expr(arena: &TreeArena, def: TreeId) -> TreeId {
    let payload = list_tail(arena, def);
    list_tail(arena, payload)
}

fn flatten_top_par(arena: &TreeArena, expr: TreeId, out: &mut Vec<TreeId>) {
    if let Some((left, right)) = boxes::is_box_par(arena, expr) {
        out.push(left);
        flatten_top_par(arena, right, out);
    } else {
        out.push(expr);
    }
}

#[test]
fn supports_with_local_def_expression() {
    let output = parse_program("process = _ with { a = _; };", "slice6_with.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    assert!(is_box_with_local_def(&output.state.arena, expr).is_some());
}

#[test]
fn supports_letrec_expression() {
    let output = parse_program("process = _ letrec { 'x = _; };", "slice6_letrec.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let (_body, rec_defs, where_defs) = is_box_with_rec_def(&output.state.arena, expr)
        .expect("letrec should produce box_with_rec_def");
    assert!(!output.state.arena.is_nil(rec_defs));
    assert!(output.state.arena.is_nil(where_defs));
}

#[test]
fn supports_environment_component_and_library_primitives() {
    let output = parse_program(
        r#"process = environment { a = _; }, component("foo.dsp"), library("bar.lib");"#,
        "slice6_modules.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let mut elems = Vec::new();
    flatten_top_par(&output.state.arena, expr, &mut elems);
    assert_eq!(elems.len(), 3);

    let env_with_def = elems[0];
    let component = elems[1];
    let library = elems[2];
    let (env, _defs) = is_box_with_local_def(&output.state.arena, env_with_def)
        .expect("environment should lower to box_with_local_def");
    assert!(is_box_environment(&output.state.arena, env));
    assert!(is_box_component(&output.state.arena, component).is_some());
    assert!(is_box_library(&output.state.arena, library).is_some());
}

#[test]
fn supports_waveform_and_route_primitives() {
    let output = parse_program(
        "process = route(_, _), route(_, _, _), waveform { 1, -2, 3.5 };",
        "slice6_wave_route.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let mut elems = Vec::new();
    flatten_top_par(&output.state.arena, expr, &mut elems);
    assert_eq!(elems.len(), 3);
    let route2 = elems[0];
    let route3 = elems[1];
    let waveform_expr = elems[2];

    let (_, _, fake_spec) =
        is_box_route(&output.state.arena, route2).expect("route(_,_) should parse");
    assert_eq!(
        dump_box(&output.state.arena, fake_spec),
        "BOXPAR(int(0), int(0))"
    );
    assert!(is_box_route(&output.state.arena, route3).is_some());

    let wave_list =
        is_box_waveform(&output.state.arena, waveform_expr).expect("waveform should parse");
    let v0 = output.state.arena.hd(wave_list).expect("v0");
    let t1 = output.state.arena.tl(wave_list).expect("tail1");
    let v1 = output.state.arena.hd(t1).expect("v1");
    let t2 = output.state.arena.tl(t1).expect("tail2");
    let v2 = output.state.arena.hd(t2).expect("v2");
    assert!(matches!(
        output.state.arena.kind(v0),
        Some(NodeKind::Int(1))
    ));
    assert!(matches!(
        output.state.arena.kind(v1),
        Some(NodeKind::Int(-2))
    ));
    assert!(is_box_real(&output.state.arena, v2));
}
