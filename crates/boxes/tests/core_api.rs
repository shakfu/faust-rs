use boxes::{
    box_cut, box_environment, box_hslider, box_ident, box_ident_name, box_int, box_ipar, box_merge,
    box_par, box_real, box_rec, box_seq, box_split, box_wire, box_with_local_def, box_with_rec_def,
    is_box_cut, is_box_environment, is_box_hslider, is_box_int, is_box_ipar, is_box_merge,
    is_box_par, is_box_real, is_box_rec, is_box_seq, is_box_split, is_box_wire,
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
fn hslider_preserves_faust_list4_layout() {
    let mut arena = TreeArena::new();
    let label = box_ident(&mut arena, "freq");
    let cur = box_real(&mut arena, 440.0);
    let min = box_real(&mut arena, 20.0);
    let max = box_real(&mut arena, 20_000.0);
    let step = box_real(&mut arena, 1.0);
    let slider = box_hslider(&mut arena, label, cur, min, max, step);

    assert_eq!(
        is_box_hslider(&arena, slider),
        Some((label, cur, min, max, step))
    );
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
