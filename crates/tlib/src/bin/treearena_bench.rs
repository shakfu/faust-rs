use std::env;
use std::hint::black_box;
use std::time::Instant;

use tlib::{NodeKind, PropertyStore, TreeArena};

fn parse_size() -> usize {
    env::args()
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(200_000)
}

fn main() {
    let n = parse_size();

    let mut arena = TreeArena::new();
    let mut nodes = Vec::with_capacity(n);

    let create_start = Instant::now();
    for i in 0..n {
        let a = arena.int(i as i64);
        let b = arena.int((i as i64) + 1);
        let node = arena.intern(NodeKind::Tag("pair".to_owned()), &[a, b]);
        nodes.push(node);
    }
    let create_elapsed = create_start.elapsed();

    let lookup_start = Instant::now();
    for i in 0..n {
        let a = arena.int(i as i64);
        let b = arena.int((i as i64) + 1);
        let node = arena.intern(NodeKind::Tag("pair".to_owned()), &[a, b]);
        black_box(node);
    }
    let lookup_elapsed = lookup_start.elapsed();

    let traversal_start = Instant::now();
    let mut list = arena.nil();
    for node in nodes.iter().copied() {
        list = arena.cons(node, list);
    }
    let mut count = 0usize;
    let mut cur = list;
    while !arena.is_nil(cur) {
        count += 1;
        cur = arena.tl(cur).expect("proper cons list");
    }
    let traversal_elapsed = traversal_start.elapsed();
    black_box(count);

    let mut props = PropertyStore::<usize>::new();
    let hot_key = props.key("hot");
    let prop_set_start = Instant::now();
    for (i, node) in nodes.iter().copied().enumerate() {
        let _ = props.set_with_key(node, hot_key, i);
    }
    let prop_set_elapsed = prop_set_start.elapsed();

    let prop_get_start = Instant::now();
    let mut checksum = 0usize;
    for node in nodes.iter().copied() {
        if let Some(v) = props.get_with_key(node, hot_key) {
            checksum ^= *v;
        }
    }
    let prop_get_elapsed = prop_get_start.elapsed();
    black_box(checksum);

    println!("TreeArena micro-bench (n={n})");
    println!("create_ms={:.3}", create_elapsed.as_secs_f64() * 1_000.0);
    println!("lookup_ms={:.3}", lookup_elapsed.as_secs_f64() * 1_000.0);
    println!(
        "traversal_ms={:.3}",
        traversal_elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "property_set_ms={:.3}",
        prop_set_elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "property_get_ms={:.3}",
        prop_get_elapsed.as_secs_f64() * 1_000.0
    );
    println!("arena_nodes={}", arena.len());
}
