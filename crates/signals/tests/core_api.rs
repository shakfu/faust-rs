//! Integration tests for `core_api`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

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
fn ui_shapes_decode_with_control_ids() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let sig = b.input(0);
    let slider = 7;
    let bargraph = 8;
    let soundfile = 9;

    let vs = b.vslider(slider);
    let hs = b.hslider(slider);
    let ne = b.numentry(slider);
    let vb = b.vbargraph(bargraph, sig);
    let hb = b.hbargraph(bargraph, sig);
    let sf = b.soundfile(soundfile);
    let sf_part = b.int(2);
    let sf_chan = b.int(1);
    let sf_ridx = b.input(3);
    let sf_len = b.soundfile_length(sf, sf_part);
    let sf_rate = b.soundfile_rate(sf, sf_part);
    let sf_buf = b.soundfile_buffer(sf, sf_chan, sf_part, sf_ridx);

    assert_eq!(match_sig(&arena, vs), SigMatch::VSlider(slider));
    assert_eq!(match_sig(&arena, hs), SigMatch::HSlider(slider));
    assert_eq!(match_sig(&arena, ne), SigMatch::NumEntry(slider));
    assert_eq!(match_sig(&arena, vb), SigMatch::VBargraph(bargraph, sig));
    assert_eq!(match_sig(&arena, hb), SigMatch::HBargraph(bargraph, sig));
    assert_eq!(match_sig(&arena, sf), SigMatch::Soundfile(soundfile));
    assert_eq!(
        match_sig(&arena, sf_len),
        SigMatch::SoundfileLength(sf, sf_part)
    );
    assert_eq!(
        match_sig(&arena, sf_rate),
        SigMatch::SoundfileRate(sf, sf_part)
    );
    assert_eq!(
        match_sig(&arena, sf_buf),
        SigMatch::SoundfileBuffer(sf, sf_chan, sf_part, sf_ridx)
    );
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
    let tv = b.temp_var(x);
    let pv = b.perm_var(y);
    let clk = b.int(7);
    let clocked = b.clocked(clk, x);
    let double_clocked = b.double_clocked(clk, y, x);
    let rec = b.rec(x);
    let reverse_rec = b.reverse_time_rec(y);
    let proj = b.proj(2, rec);
    let seq = b.seq(x, y);
    let zp = b.zero_pad(x, y);

    assert!(matches!(match_sig(&arena, od), SigMatch::OnDemand(v) if v == [x, y]));
    assert!(matches!(match_sig(&arena, us), SigMatch::Upsampling(v) if v == [x, y]));
    assert!(matches!(match_sig(&arena, ds), SigMatch::Downsampling(v) if v == [x, y]));
    assert_eq!(match_sig(&arena, tv), SigMatch::TempVar(x));
    assert_eq!(match_sig(&arena, pv), SigMatch::PermVar(y));
    assert_eq!(match_sig(&arena, clocked), SigMatch::Clocked(clk, x));
    let SigMatch::Clocked(inner_clk, nested) = match_sig(&arena, double_clocked) else {
        panic!("double_clocked should decode as outer clocked");
    };
    assert_eq!(inner_clk, clk);
    assert_eq!(match_sig(&arena, nested), SigMatch::Clocked(y, x));
    assert_eq!(match_sig(&arena, rec), SigMatch::Rec(x));
    assert_eq!(match_sig(&arena, reverse_rec), SigMatch::ReverseTimeRec(y));
    assert_eq!(match_sig(&arena, proj), SigMatch::Proj(2, rec));
    assert_eq!(match_sig(&arena, seq), SigMatch::Seq(x, y));
    assert_eq!(match_sig(&arena, zp), SigMatch::ZeroPad(x, y));
}

#[test]
fn dump_is_deterministic() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let btn = b.button(1);
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

#[test]
fn dump_readable_handles_deep_left_spines_without_recursing() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let mut sig = b.input(0);
    for _ in 0..20_000 {
        let zero = b.int(0);
        sig = b.add(sig, zero);
    }

    let got = dump_sig_readable(&arena, sig);
    assert!(got.starts_with("SIGBINOP(op=add (+),"));
    assert!(got.contains("SIGINPUT(int(0))"));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "SIGINPUT index must be non-negative")]
fn builder_rejects_negative_input_index_in_debug() {
    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let _ = b.input(-1);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "SIGOUTPUT index must be non-negative")]
fn builder_rejects_negative_output_index_in_debug() {
    let mut arena = TreeArena::new();
    let input = {
        let mut b = SigBuilder::new(&mut arena);
        b.input(0)
    };
    let mut b = SigBuilder::new(&mut arena);
    let _ = b.output(-1, input);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "SIGPROJ index must be non-negative")]
fn builder_rejects_negative_proj_index_in_debug() {
    let mut arena = TreeArena::new();
    let input = {
        let mut b = SigBuilder::new(&mut arena);
        b.input(0)
    };
    let rec = {
        let mut b = SigBuilder::new(&mut arena);
        b.rec(input)
    };
    let mut b = SigBuilder::new(&mut arena);
    let _ = b.proj(-1, rec);
}
