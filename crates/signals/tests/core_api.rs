//! Integration tests for core_api.rs.

use signals::{BinOp, SigBuilder, SigMatch, dump_sig, dump_sig_readable, match_sig};
use tlib::TreeArena;

#[test]
fn builder_and_match_cover_core_signal_shapes() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);

    let i0 = b.int(0);
    let i1 = b.int(1);
    let r = b.real(0.25);
    let inp = b.input(3);
    let out = b.output(2, inp);
    let d1 = b.delay1(inp);
    let d = b.delay(inp, i1);
    let p = b.prefix(i0, inp);
    let add = b.add(inp, i1);
    let pow = b.pow(inp, i1);
    let min = b.min(inp, i1);
    let max = b.max(inp, i1);
    let acos = b.acos(inp);
    let asin = b.asin(inp);
    let atan = b.atan(inp);
    let atan2 = b.atan2(inp, i1);
    let cos = b.cos(inp);
    let sin = b.sin(inp);
    let tan = b.tan(inp);
    let exp = b.exp(inp);
    let log = b.log(inp);
    let log10 = b.log10(inp);
    let sqrt = b.sqrt(inp);
    let abs = b.abs(inp);
    let fmod = b.fmod(inp, i1);
    let remainder = b.remainder(inp, i1);
    let floor = b.floor(inp);
    let ceil = b.ceil(inp);
    let rint = b.rint(inp);
    let round = b.round(inp);
    let cast_i = b.int_cast(r);
    let cast_f = b.float_cast(i1);

    assert_eq!(match_sig(&arena, i1), SigMatch::Int(1));
    assert!(matches!(match_sig(&arena, r), SigMatch::Real(_)));
    assert_eq!(match_sig(&arena, inp), SigMatch::Input(3));
    assert_eq!(match_sig(&arena, out), SigMatch::Output(2, inp));
    assert_eq!(match_sig(&arena, d1), SigMatch::Delay1(inp));
    assert_eq!(match_sig(&arena, d), SigMatch::Delay(inp, i1));
    assert_eq!(match_sig(&arena, p), SigMatch::Prefix(i0, inp));
    assert_eq!(match_sig(&arena, add), SigMatch::BinOp(BinOp::Add, inp, i1));
    assert_eq!(match_sig(&arena, pow), SigMatch::Pow(inp, i1));
    assert_eq!(match_sig(&arena, min), SigMatch::Min(inp, i1));
    assert_eq!(match_sig(&arena, max), SigMatch::Max(inp, i1));
    assert_eq!(match_sig(&arena, acos), SigMatch::Acos(inp));
    assert_eq!(match_sig(&arena, asin), SigMatch::Asin(inp));
    assert_eq!(match_sig(&arena, atan), SigMatch::Atan(inp));
    assert_eq!(match_sig(&arena, atan2), SigMatch::Atan2(inp, i1));
    assert_eq!(match_sig(&arena, cos), SigMatch::Cos(inp));
    assert_eq!(match_sig(&arena, sin), SigMatch::Sin(inp));
    assert_eq!(match_sig(&arena, tan), SigMatch::Tan(inp));
    assert_eq!(match_sig(&arena, exp), SigMatch::Exp(inp));
    assert_eq!(match_sig(&arena, log), SigMatch::Log(inp));
    assert_eq!(match_sig(&arena, log10), SigMatch::Log10(inp));
    assert_eq!(match_sig(&arena, sqrt), SigMatch::Sqrt(inp));
    assert_eq!(match_sig(&arena, abs), SigMatch::Abs(inp));
    assert_eq!(match_sig(&arena, fmod), SigMatch::Fmod(inp, i1));
    assert_eq!(match_sig(&arena, remainder), SigMatch::Remainder(inp, i1));
    assert_eq!(match_sig(&arena, floor), SigMatch::Floor(inp));
    assert_eq!(match_sig(&arena, ceil), SigMatch::Ceil(inp));
    assert_eq!(match_sig(&arena, rint), SigMatch::Rint(inp));
    assert_eq!(match_sig(&arena, round), SigMatch::Round(inp));
    assert_eq!(match_sig(&arena, cast_i), SigMatch::IntCast(r));
    assert!(matches!(match_sig(&arena, cast_f), SigMatch::Real(_)));
}

#[test]
fn table_wrappers_and_select3_follow_cpp_shape() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let size = b.int(64);
    let init = b.int(7);
    let widx = b.int(4);
    let wsig = b.int(9);
    let ridx = b.int(5);

    let gen_init = b.generate(init);
    let wr = b.wrtbl(size, gen_init, widx, wsig);
    let ro = b.read_only_table(size, init, ridx);
    let rw = b.write_read_table(size, init, widx, wsig, ridx);
    let sidx = b.int(0);
    let s1 = b.int(10);
    let s2 = b.int(11);
    let s3 = b.int(12);
    let sel3 = b.select3(sidx, s1, s2, s3);

    assert!(matches!(match_sig(&arena, wr), SigMatch::WrTbl(_, _, _, _)));
    assert!(matches!(match_sig(&arena, ro), SigMatch::RdTbl(_, _)));
    assert!(matches!(match_sig(&arena, rw), SigMatch::RdTbl(_, _)));
    assert!(matches!(
        match_sig(&arena, sel3),
        SigMatch::Select2(_, _, _)
    ));
}

#[test]
fn ui_shapes_decode_with_list4_payload_order() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let label = b.int(123);
    let init = b.real(0.5);
    let min = b.int(0);
    let max = b.int(1);
    let step = b.real(0.01);
    let sig = b.input(0);

    let vs = b.vslider(label, init, min, max, step);
    let hs = b.hslider(label, init, min, max, step);
    let ne = b.numentry(label, init, min, max, step);
    let vb = b.vbargraph(label, min, max, sig);
    let hb = b.hbargraph(label, min, max, sig);
    let sf = b.soundfile(label);

    assert_eq!(
        match_sig(&arena, vs),
        SigMatch::VSlider(label, init, min, max, step)
    );
    assert_eq!(
        match_sig(&arena, hs),
        SigMatch::HSlider(label, init, min, max, step)
    );
    assert_eq!(
        match_sig(&arena, ne),
        SigMatch::NumEntry(label, init, min, max, step)
    );
    assert_eq!(
        match_sig(&arena, vb),
        SigMatch::VBargraph(label, min, max, sig)
    );
    assert_eq!(
        match_sig(&arena, hb),
        SigMatch::HBargraph(label, min, max, sig)
    );
    assert_eq!(match_sig(&arena, sf), SigMatch::Soundfile(label));
}

#[test]
fn stream_wrappers_and_recursion_shapes_are_stable() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let x = b.input(0);
    let y = b.input(1);

    let od = b.on_demand(&[x, y]);
    let us = b.upsampling(&[x, y]);
    let ds = b.downsampling(&[x, y]);
    let rec = b.rec(x);
    let proj = b.proj(2, rec);
    let seq = b.seq(x, y);
    let zp = b.zero_pad(x, y);

    assert!(matches!(match_sig(&arena, od), SigMatch::OnDemand(v) if v == [x, y]));
    assert!(matches!(match_sig(&arena, us), SigMatch::Upsampling(v) if v == [x, y]));
    assert!(matches!(match_sig(&arena, ds), SigMatch::Downsampling(v) if v == [x, y]));
    assert_eq!(match_sig(&arena, rec), SigMatch::Rec(x));
    assert_eq!(match_sig(&arena, proj), SigMatch::Proj(2, rec));
    assert_eq!(match_sig(&arena, seq), SigMatch::Seq(x, y));
    assert_eq!(match_sig(&arena, zp), SigMatch::ZeroPad(x, y));
}

#[test]
fn dump_is_deterministic() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let one = b.int(1);
    let btn = b.button(one);
    let inp = b.input(0);
    let d1 = b.delay1(inp);
    let sig = b.attach(btn, d1);
    let a = dump_sig(&arena, sig);
    let c = dump_sig(&arena, sig);
    assert_eq!(a, c);
}

#[test]
fn dump_readable_prints_binop_opcode_name() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let x = b.input(0);
    let y = b.input(1);
    let add = b.add(x, y);
    let got = dump_sig_readable(&arena, add);
    assert!(got.contains("SIGBINOP(op=add (+),"));
}
