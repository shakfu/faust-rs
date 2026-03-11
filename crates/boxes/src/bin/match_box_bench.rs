//! Micro-benchmark for `boxes::match_box` dispatch performance.
//!
//! # Scope
//! - Builds representative node sets for the active `BoxMatch` families.
//! - Measures hot-path matching throughput over repeated decode rounds.
//! - Prints per-case timings to support local dispatch regression tracking.

use std::hint::black_box;
use std::mem::discriminant;
use std::time::Instant;

use boxes::{BoxBuilder, BoxId, match_box};
use tlib::TreeArena;

/// Runs one named `match_box` throughput benchmark case.
fn bench_case(name: &str, arena: &TreeArena, nodes: &[BoxId], rounds: usize) {
    let start = Instant::now();
    let mut sink: u64 = 0;
    for _ in 0..rounds {
        for &id in nodes {
            let m = black_box(match_box(arena, black_box(id)));
            sink = sink.wrapping_add(discriminant(&m).hash64());
        }
    }
    let elapsed = start.elapsed();
    let ops = (nodes.len() * rounds) as f64;
    let ns_per_op = elapsed.as_secs_f64() * 1e9 / ops;
    let mops = ops / elapsed.as_secs_f64() / 1e6;
    println!("{name:16}  ops={ops:.0}  ns/op={ns_per_op:.2}  Mops/s={mops:.2}  sink={sink}");
}

/// Small helper trait to fold enum discriminants into the benchmark sink.
trait DiscriminantHash64 {
    /// Hashes the discriminant to a stable `u64` sink value.
    fn hash64(&self) -> u64;
}

impl<T> DiscriminantHash64 for std::mem::Discriminant<T> {
    /// Hashes the discriminant with the default hasher.
    fn hash64(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(self, &mut h);
        std::hash::Hasher::finish(&h)
    }
}

/// Builds representative box families and prints local throughput timings.
fn main() {
    let mut arena = TreeArena::new();
    let i0 = arena.int(0);
    let i1 = arena.int(1);
    let r05 = arena.float(0.5);
    let nil = arena.nil();
    let arglist = arena.cons(i1, nil);
    let sym = arena.symbol("sym");
    let arglist_tail = arena.cons(arglist, nil);
    let signature = arena.cons(i1, arglist_tail);
    let route_spec = {
        let mut b = BoxBuilder::new(&mut arena);
        b.par(i0, i0)
    };
    let case_wire = {
        let mut b = BoxBuilder::new(&mut arena);
        b.wire()
    };
    let case_rules = {
        let rule = arena.cons(arglist, case_wire);
        arena.cons(rule, nil)
    };

    let mut prims = Vec::new();
    for _ in 0..1024 {
        let mut b = BoxBuilder::new(&mut arena);
        prims.extend_from_slice(&[
            b.add(),
            b.sub(),
            b.mul(),
            b.div(),
            b.rem(),
            b.and(),
            b.or(),
            b.xor(),
            b.lsh(),
            b.rsh(),
            b.lt(),
            b.le(),
            b.gt(),
            b.ge(),
            b.eq(),
            b.ne(),
            b.pow(),
            b.delay(),
            b.delay1(),
            b.min(),
            b.max(),
            b.prefix(),
            b.int_cast(),
            b.float_cast(),
            b.read_only_table(),
            b.write_read_table(),
            b.select2(),
            b.select3(),
            b.assert_bounds(),
            b.lowest(),
            b.highest(),
            b.attach(),
            b.enable(),
            b.control(),
        ]);
    }

    let mut sliders = Vec::new();
    {
        let mut b = BoxBuilder::new(&mut arena);
        for _ in 0..4096 {
            sliders.push(b.vslider(sym, r05, i0, i1, r05));
            sliders.push(b.hslider(sym, r05, i0, i1, r05));
            sliders.push(b.num_entry(sym, r05, i0, i1, r05));
        }
    }

    let mut mixed = Vec::new();
    for _ in 0..1024 {
        let mut b = BoxBuilder::new(&mut arena);
        let wire = b.wire();
        let ident = b.ident("x");
        let ff = b.ffunction(signature, sym, sym);
        mixed.extend_from_slice(&[
            ident,
            i1,
            r05,
            wire,
            b.cut(),
            b.seq(wire, i1),
            b.par(wire, i1),
            b.rec(wire, i1),
            b.split(wire, i1),
            b.merge(wire, i1),
            b.appl(ident, arglist),
            b.access(ident, sym),
            b.ipar(ident, i1, wire),
            b.iseq(ident, i1, wire),
            b.isum(ident, i1, wire),
            b.iprod(ident, i1, wire),
            b.with_local_def(wire, arglist),
            b.with_rec_def(wire, arglist, arglist),
            b.environment(),
            b.component(sym),
            b.library(sym),
            b.waveform(&[i0, i1, r05]),
            b.route(i1, i1, route_spec),
            ff,
            b.ffun(ff),
            b.fconst(i1, sym, sym),
            b.fvar(i1, sym, sym),
            b.case(case_rules),
            b.pattern_var(ident),
            b.abstr(ident, wire),
            b.modulation(ident, wire),
            b.inputs(wire),
            b.outputs(wire),
            b.ondemand(wire),
            b.upsampling(wire),
            b.downsampling(wire),
            b.button(sym),
            b.checkbox(sym),
            b.vgroup(sym, wire),
            b.hgroup(sym, wire),
            b.tgroup(sym, wire),
            b.vbargraph(sym, i0, i1),
            b.hbargraph(sym, i0, i1),
            b.soundfile(sym, i0),
        ]);
    }

    println!("match_box benchmark (release)");
    bench_case("primitives", &arena, &prims, 200);
    bench_case("sliders", &arena, &sliders, 200);
    bench_case("mixed", &arena, &mixed, 200);
}
