use boxes::{
    box_access, box_add, box_appl, box_button, box_checkbox, box_component, box_cut, box_delay1,
    box_environment, box_hbargraph, box_hslider, box_ident, box_ident_name, box_int, box_ipar,
    box_iprod, box_iseq, box_isum, box_library, box_max, box_merge, box_min, box_mul,
    box_num_entry, box_par, box_real, box_rec, box_route, box_seq, box_split, box_vbargraph,
    box_vslider, box_waveform, box_wire, box_with_local_def, box_with_rec_def, dump_box,
    is_box_access, is_box_add, is_box_appl, is_box_button, is_box_checkbox, is_box_component,
    is_box_cut, is_box_delay1, is_box_environment, is_box_hbargraph, is_box_hslider, is_box_int,
    is_box_ipar, is_box_iprod, is_box_iseq, is_box_isum, is_box_library, is_box_max, is_box_merge,
    is_box_min, is_box_mul, is_box_num_entry, is_box_par, is_box_real, is_box_rec, is_box_route,
    is_box_seq, is_box_split, is_box_vbargraph, is_box_vslider, is_box_waveform, is_box_wire,
    is_box_with_local_def, is_box_with_rec_def,
};
use tlib::TreeArena;

#[test]
fn ident_and_numeric_boxes_match_expected_kinds() {
    let mut arena = TreeArena::new();
    let ident = box_ident(&mut arena, "freq");
    assert_eq!(box_ident_name(&arena, ident), Some("freq"));

    let i = box_int(&mut arena, 42);
    let r = box_real(&mut arena, 0.5);
    assert!(is_box_int(&arena, i));
    assert!(!is_box_real(&arena, i));
    assert!(is_box_real(&arena, r));
    assert!(!is_box_int(&arena, r));
}

#[test]
fn basic_composition_boxes_roundtrip() {
    let mut arena = TreeArena::new();
    let a = box_wire(&mut arena);
    let b = box_cut(&mut arena);

    let seq = box_seq(&mut arena, a, b);
    let par = box_par(&mut arena, a, b);
    let rec = box_rec(&mut arena, a, b);
    let spl = box_split(&mut arena, a, b);
    let mer = box_merge(&mut arena, a, b);

    assert_eq!(is_box_seq(&arena, seq), Some((a, b)));
    assert_eq!(is_box_par(&arena, par), Some((a, b)));
    assert_eq!(is_box_rec(&arena, rec), Some((a, b)));
    assert_eq!(is_box_split(&arena, spl), Some((a, b)));
    assert_eq!(is_box_merge(&arena, mer), Some((a, b)));
}

#[test]
fn primitive_appl_and_access_boxes_roundtrip() {
    let mut arena = TreeArena::new();
    let one = box_int(&mut arena, 1);
    let two = box_int(&mut arena, 2);
    let nil = arena.nil();
    let tail = arena.cons(one, nil);
    let rev_args = arena.cons(two, tail);
    let fun = box_ident(&mut arena, "f");

    let appl = box_appl(&mut arena, fun, rev_args);
    assert_eq!(is_box_appl(&arena, appl), Some((fun, rev_args)));

    let field = box_ident(&mut arena, "bar");
    let acc = box_access(&mut arena, fun, field);
    assert_eq!(is_box_access(&arena, acc), Some((fun, field)));

    let add = box_add(&mut arena);
    let mul = box_mul(&mut arena);
    let delay1 = box_delay1(&mut arena);
    let min = box_min(&mut arena);
    let max = box_max(&mut arena);
    assert!(is_box_add(&arena, add));
    assert!(is_box_mul(&arena, mul));
    assert!(is_box_delay1(&arena, delay1));
    assert!(is_box_min(&arena, min));
    assert!(is_box_max(&arena, max));
}

#[test]
fn wire_cut_environment_predicates_are_stable() {
    let mut arena = TreeArena::new();
    let w1 = box_wire(&mut arena);
    let w2 = box_wire(&mut arena);
    let c = box_cut(&mut arena);
    let env = box_environment(&mut arena);

    // Hash-consing parity: same primitive constructor gives same node id.
    assert_eq!(w1, w2);
    assert!(is_box_wire(&arena, w1));
    assert!(is_box_cut(&arena, c));
    assert!(is_box_environment(&arena, env));
}

#[test]
fn ipar_roundtrip_preserves_argument_order() {
    let mut arena = TreeArena::new();
    let idx = box_int(&mut arena, 0);
    let count = box_int(&mut arena, 4);
    let body = box_wire(&mut arena);
    let ipar = box_ipar(&mut arena, idx, count, body);

    assert_eq!(is_box_ipar(&arena, ipar), Some((idx, count, body)));
}

#[test]
fn iterative_compositions_roundtrip_preserve_argument_order() {
    let mut arena = TreeArena::new();
    let idx = box_int(&mut arena, 0);
    let count = box_int(&mut arena, 4);
    let body = box_wire(&mut arena);

    let iseq = box_iseq(&mut arena, idx, count, body);
    let isum = box_isum(&mut arena, idx, count, body);
    let iprod = box_iprod(&mut arena, idx, count, body);

    assert_eq!(is_box_iseq(&arena, iseq), Some((idx, count, body)));
    assert_eq!(is_box_isum(&arena, isum), Some((idx, count, body)));
    assert_eq!(is_box_iprod(&arena, iprod), Some((idx, count, body)));
}

#[test]
fn ui_widgets_preserve_expected_layouts() {
    let mut arena = TreeArena::new();
    let label = box_ident(&mut arena, "freq");
    let cur = box_real(&mut arena, 440.0);
    let min = box_real(&mut arena, 20.0);
    let max = box_real(&mut arena, 20_000.0);
    let step = box_real(&mut arena, 1.0);
    let hslider = box_hslider(&mut arena, label, cur, min, max, step);
    let vslider = box_vslider(&mut arena, label, cur, min, max, step);
    let nentry = box_num_entry(&mut arena, label, cur, min, max, step);

    assert_eq!(
        is_box_hslider(&arena, hslider),
        Some((label, cur, min, max, step))
    );
    assert_eq!(
        is_box_vslider(&arena, vslider),
        Some((label, cur, min, max, step))
    );
    assert_eq!(
        is_box_num_entry(&arena, nentry),
        Some((label, cur, min, max, step))
    );

    let button = box_button(&mut arena, label);
    let checkbox = box_checkbox(&mut arena, label);
    assert_eq!(is_box_button(&arena, button), Some(label));
    assert_eq!(is_box_checkbox(&arena, checkbox), Some(label));

    let hbar = box_hbargraph(&mut arena, label, min, max);
    let vbar = box_vbargraph(&mut arena, label, min, max);
    assert_eq!(is_box_hbargraph(&arena, hbar), Some((label, min, max)));
    assert_eq!(is_box_vbargraph(&arena, vbar), Some((label, min, max)));
}

#[test]
fn local_and_recursive_def_boxes_roundtrip() {
    let mut arena = TreeArena::new();
    let body = box_wire(&mut arena);
    let a_ident = box_ident(&mut arena, "a");
    let a_value = box_int(&mut arena, 1);
    let ldef = box_par(&mut arena, a_ident, a_value);
    let local = box_with_local_def(&mut arena, body, ldef);
    assert_eq!(is_box_with_local_def(&arena, local), Some((body, ldef)));

    let b_ident = box_ident(&mut arena, "b");
    let b_value = box_int(&mut arena, 2);
    let ldef2 = box_par(&mut arena, b_ident, b_value);
    let rec = box_with_rec_def(&mut arena, body, ldef, ldef2);
    assert_eq!(is_box_with_rec_def(&arena, rec), Some((body, ldef, ldef2)));
}

#[test]
fn module_waveform_and_route_boxes_roundtrip() {
    let mut arena = TreeArena::new();
    let filename = arena.string_lit("foo.lib");
    let component = box_component(&mut arena, filename);
    let library = box_library(&mut arena, filename);
    assert_eq!(is_box_component(&arena, component), Some(filename));
    assert_eq!(is_box_library(&arena, library), Some(filename));

    let v0 = box_int(&mut arena, 1);
    let v1 = box_int(&mut arena, -2);
    let v2 = box_real(&mut arena, 3.5);
    let values = [v0, v1, v2];
    let waveform = box_waveform(&mut arena, &values);
    let wave_list = is_box_waveform(&arena, waveform).expect("waveform payload should exist");
    assert_eq!(arena.hd(wave_list), Some(values[0]));
    let wave_tail = arena.tl(wave_list).expect("tail should exist");
    assert_eq!(arena.hd(wave_tail), Some(values[1]));

    let n = box_wire(&mut arena);
    let m = box_wire(&mut arena);
    let rz0 = box_int(&mut arena, 0);
    let rz1 = box_int(&mut arena, 0);
    let spec = box_par(&mut arena, rz0, rz1);
    let route = box_route(&mut arena, n, m, spec);
    assert_eq!(is_box_route(&arena, route), Some((n, m, spec)));
}

#[test]
fn structural_dump_is_deterministic_and_id_free() {
    let mut arena = TreeArena::new();
    let label = box_ident(&mut arena, "gain");
    let cur = box_real(&mut arena, 0.5);
    let min = box_real(&mut arena, 0.0);
    let max = box_real(&mut arena, 1.0);
    let step = box_real(&mut arena, 0.01);
    let slider = box_hslider(&mut arena, label, cur, min, max, step);
    let wire = box_wire(&mut arena);
    let root = box_seq(&mut arena, wire, slider);

    let dump_a = dump_box(&arena, root);
    let dump_b = dump_box(&arena, root);
    assert_eq!(dump_a, dump_b);
    assert_eq!(
        dump_a,
        "BOXSEQ(BOXWIRE(), BOXHSLIDER(BOXIDENT(sym(\"gain\")), cons(float_bits(0x3fe0000000000000), cons(float_bits(0x0000000000000000), cons(float_bits(0x3ff0000000000000), cons(float_bits(0x3f847ae147ae147b), nil))))))"
    );
    assert!(!dump_a.contains("TreeId("));
}
