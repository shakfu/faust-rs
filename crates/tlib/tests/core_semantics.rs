use tlib::{NodeKind, PropertyStore, TreeArena};

#[test]
fn interning_reuses_structurally_identical_nodes() {
    let mut arena = TreeArena::new();
    let a = arena.symbol("x");
    let b = arena.symbol("x");
    assert_eq!(a, b);

    let seq1 = arena.intern(NodeKind::Tag("seq".to_owned()), &[a, arena.nil()]);
    let seq2 = arena.intern(NodeKind::Tag("seq".to_owned()), &[b, arena.nil()]);
    assert_eq!(seq1, seq2);
}

#[test]
fn list_operations_preserve_head_and_tail_order() {
    let mut arena = TreeArena::new();
    let one = arena.int(1);
    let two = arena.int(2);
    let nil = arena.nil();

    let tail = arena.cons(two, nil);
    let list = arena.cons(one, tail);

    assert!(arena.is_list(list));
    assert!(!arena.is_nil(list));
    assert_eq!(arena.hd(list), Some(one));

    let next = arena.tl(list).expect("cons tail should exist");
    assert_eq!(arena.hd(next), Some(two));
    assert_eq!(arena.tl(next), Some(nil));
    assert!(arena.is_nil(nil));
}

#[test]
fn property_store_is_node_keyed() {
    let mut arena = TreeArena::new();
    let node = arena.symbol("gain");
    let other = arena.symbol("freq");

    let mut props = PropertyStore::<i32>::new();
    assert!(props.is_empty());

    assert_eq!(props.set(node, "order", 3), None);
    assert_eq!(props.get(node, "order"), Some(&3));
    assert_eq!(props.get(other, "order"), None);

    if let Some(value) = props.get_mut(node, "order") {
        *value += 1;
    }
    assert_eq!(props.get(node, "order"), Some(&4));

    assert_eq!(props.remove(node, "order"), Some(4));
    assert!(props.get(node, "order").is_none());
}

#[test]
fn property_store_interned_key_api_matches_string_api() {
    let mut arena = TreeArena::new();
    let node = arena.symbol("gain");

    let mut props = PropertyStore::<i32>::new();
    let order = props.key("order");

    assert_eq!(props.set_with_key(node, order, 7), None);
    assert_eq!(props.get_with_key(node, order), Some(&7));
    assert_eq!(props.get(node, "order"), Some(&7));

    if let Some(value) = props.get_mut_with_key(node, order) {
        *value += 2;
    }
    assert_eq!(props.get(node, "order"), Some(&9));
    assert_eq!(props.remove_with_key(node, order), Some(9));
}
