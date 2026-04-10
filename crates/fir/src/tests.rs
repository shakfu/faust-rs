use super::*;

#[test]
fn builder_and_match_cover_core_value_nodes() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let one = b.int32(1);
    let two = b.int32(2);
    let sum = b.binop(FirBinOp::Add, one, two, FirType::Int32);
    let call = b.fun_call("foo", &[sum], FirType::Int32);
    let cast = b.cast(FirType::Float64, call);

    assert_eq!(
        match_fir(&store, one),
        FirMatch::Int32 {
            value: 1,
            typ: FirType::Int32
        }
    );
    assert_eq!(
        match_fir(&store, sum),
        FirMatch::BinOp {
            op: FirBinOp::Add,
            lhs: one,
            rhs: two,
            typ: FirType::Int32
        }
    );
    assert_eq!(
        match_fir(&store, call),
        FirMatch::FunCall {
            name: "foo".to_string(),
            args: vec![sum],
            typ: FirType::Int32
        }
    );
    assert_eq!(
        match_fir(&store, cast),
        FirMatch::Cast {
            typ: FirType::Float64,
            value: call
        }
    );

    assert_eq!(store.value_type(cast), Some(FirType::Float64));
    assert_eq!(store.value_type(sum), Some(FirType::Int32));
}

#[test]
fn builder_and_match_cover_stmt_nodes() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let zero = b.int32(0);
    let dec = b.declare_var("acc", FirType::Int32, AccessType::Stack, Some(zero));
    let upper = b.int32(64);
    let body = b.block(&[dec]);
    let loop_ = b.simple_for_loop("i", upper, body, false);
    let ret = b.ret(Some(zero));
    let block = b.block(&[loop_, ret]);

    assert_eq!(
        match_fir(&store, dec),
        FirMatch::DeclareVar {
            name: "acc".to_string(),
            typ: FirType::Int32,
            access: AccessType::Stack,
            init: Some(zero)
        }
    );
    assert_eq!(
        match_fir(&store, loop_),
        FirMatch::SimpleForLoop {
            var: "i".to_string(),
            upper,
            body,
            is_reverse: false
        }
    );
    assert_eq!(match_fir(&store, block), FirMatch::Block(vec![loop_, ret]));
}

#[test]
fn dump_fir_expands_simple_for_loop_body() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let one = b.int32(1);
    let upper = b.int32(8);
    let body_stmt = b.store_var("acc", AccessType::Stack, one);
    let body = b.block(&[body_stmt]);
    let loop_ = b.simple_for_loop("i", upper, body, false);
    let root = b.block(&[loop_]);

    let dump = dump_fir(&store, root);
    assert!(dump.contains("SimpleForLoop"));
    assert!(dump.contains("StoreVar { name: \"acc\""));
    assert!(dump.contains("Int32 { value: 1"));
    assert!(dump.contains(&format!("#{}", body.as_u32())));
    assert!(dump.contains(&format!("#{}", body_stmt.as_u32())));
}

#[test]
fn dump_fir_expands_for_loop_body() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let zero = b.int32(0);
    let one = b.int32(1);
    let ten = b.int32(10);
    let init = b.declare_var("i", FirType::Int32, AccessType::Loop, Some(zero));
    let body_stmt = b.store_var("acc", AccessType::Stack, one);
    let body = b.block(&[body_stmt]);
    let loop_ = b.for_loop("i", init, ten, one, body, false);
    let root = b.block(&[loop_]);

    let dump = dump_fir(&store, root);
    assert!(dump.contains("ForLoop {"));
    assert!(dump.contains("StoreVar { name: \"acc\""));
    assert!(dump.contains(&format!("#{}", body.as_u32())));
    assert!(dump.contains(&format!("#{}", body_stmt.as_u32())));
}

#[test]
fn reverse_array_shift_helper_emits_expected_loop_shape() {
    let mut store = FirStore::new();
    let mut next_loop_var_id = 0;
    let loop_ = helpers::emit_reverse_array_shift_loop(
        &mut store,
        &mut next_loop_var_id,
        "lRec",
        "fRec0",
        3,
        FirType::Float32,
        AccessType::Struct,
    );

    assert_eq!(next_loop_var_id, 1);

    let FirMatch::ForLoop {
        var,
        init,
        end,
        step,
        body,
        is_reverse,
    } = match_fir(&store, loop_)
    else {
        panic!("expected helper to emit a ForLoop");
    };

    assert_eq!(var, "lRec0");
    assert!(is_reverse);
    assert_eq!(
        match_fir(&store, init),
        FirMatch::DeclareVar {
            name: "lRec0".to_string(),
            typ: FirType::Int32,
            access: AccessType::Loop,
            init: Some({
                let mut b = FirBuilder::new(&mut store);
                b.int32(3)
            }),
        }
    );
    assert_eq!(
        match_fir(&store, end),
        FirMatch::Int32 {
            value: 0,
            typ: FirType::Int32
        }
    );
    assert_eq!(
        match_fir(&store, step),
        FirMatch::Int32 {
            value: -1,
            typ: FirType::Int32
        }
    );
    assert!(matches!(match_fir(&store, body), FirMatch::Block(_)));
}

#[test]
fn dump_fir_expands_iterator_for_loop_body() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let one = b.int32(1);
    let body_stmt = b.store_var("acc", AccessType::Stack, one);
    let body = b.block(&[body_stmt]);
    let loop_ = b.iterator_for_loop(&["i0", "i1"], false, body);
    let root = b.block(&[loop_]);

    let dump = dump_fir(&store, root);
    assert!(dump.contains("IteratorForLoop {"));
    assert!(dump.contains("StoreVar { name: \"acc\""));
    assert!(dump.contains(&format!("#{}", body.as_u32())));
    assert!(dump.contains(&format!("#{}", body_stmt.as_u32())));
}

#[test]
fn builder_and_match_cover_ui_nodes() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let open = b.open_box(UiBoxType::Vertical, "osc");
    let slider = b.add_slider(
        SliderType::Horizontal,
        "freq",
        "fHslider0",
        SliderRange {
            init: 440.0,
            lo: 20.0,
            hi: 20_000.0,
            step: 1.0,
        },
    );
    let close = b.close_box();
    let block = b.block(&[open, slider, close]);

    assert_eq!(
        match_fir(&store, open),
        FirMatch::OpenBox {
            typ: UiBoxType::Vertical,
            label: "osc".to_string()
        }
    );
    assert_eq!(
        match_fir(&store, slider),
        FirMatch::AddSlider {
            typ: SliderType::Horizontal,
            label: "freq".to_string(),
            var: "fHslider0".to_string(),
            init: 440.0,
            lo: 20.0,
            hi: 20_000.0,
            step: 1.0
        }
    );
    assert_eq!(
        match_fir(&store, block),
        FirMatch::Block(vec![open, slider, close])
    );
}

#[test]
fn builder_and_match_cover_extended_cpp_families() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let x = b.load_var("x", AccessType::Stack, FirType::Float64);
    let neg = b.neg(x, FirType::Float64);
    let addr = b.load_var_address(
        "x",
        AccessType::Stack,
        FirType::Ptr(Box::new(FirType::Float64)),
    );
    let tee = b.tee_var("x", AccessType::Stack, neg, FirType::Float64);
    let cond = b.bool_(true);
    let sel = b.select2(cond, tee, x, FirType::Float64);
    let nullv = b.null_value(FirType::Void);
    let newdsp = b.new_dsp("MyDSP", FirType::Obj);
    let soundfile = b.add_soundfile("sf", "fSound0");

    assert_eq!(
        match_fir(&store, addr),
        FirMatch::LoadVarAddress {
            name: "x".to_string(),
            access: AccessType::Stack,
            typ: FirType::Ptr(Box::new(FirType::Float64))
        }
    );
    assert_eq!(
        match_fir(&store, sel),
        FirMatch::Select2 {
            cond,
            then_value: tee,
            else_value: x,
            typ: FirType::Float64
        }
    );
    assert_eq!(
        match_fir(&store, nullv),
        FirMatch::NullValue { typ: FirType::Void }
    );
    assert_eq!(
        match_fir(&store, newdsp),
        FirMatch::NewDsp {
            name: "MyDSP".to_string(),
            typ: FirType::Obj
        }
    );
    assert_eq!(
        match_fir(&store, soundfile),
        FirMatch::AddSoundfile {
            label: "sf".to_string(),
            url: String::new(),
            var: "fSound0".to_string()
        }
    );
}

#[test]
fn builder_and_match_cover_remaining_cpp_families() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let q = b.quad(1.25);
    let fx = b.fixed_point(0.5);
    let arr_i32 = b.int32_array(&[1, 2, 3]);
    let arr_f32 = b.float32_array(&[1.0, 2.0]);
    let arr_f64 = b.float64_array(&[3.5, 4.5]);
    let arr_q = b.quad_array(&[0.125, 0.25]);
    let arr_fx = b.fixed_point_array(&[0.75, 0.875]);
    let value_array = b.value_array(&[q, fx], FirType::Array(Box::new(FirType::Float64), 2));

    let dbi = b.declare_buffer_iterators("in", "out", 2, FirType::Float32, true, false);
    let body = b.block(&[dbi]);
    let ifor = b.iterator_for_loop(&["i", "j"], true, body);
    let sound = b.add_soundfile_with_url("sf", "stereo.wav", "fSound0");

    assert_eq!(
        match_fir(&store, q),
        FirMatch::Quad {
            value: 1.25,
            typ: FirType::Quad
        }
    );
    assert_eq!(
        match_fir(&store, fx),
        FirMatch::FixedPoint {
            value: 0.5,
            typ: FirType::FixedPoint
        }
    );
    assert_eq!(
        match_fir(&store, arr_i32),
        FirMatch::Int32Array {
            values: vec![1, 2, 3],
            typ: FirType::Array(Box::new(FirType::Int32), 3)
        }
    );
    assert_eq!(
        match_fir(&store, arr_f32),
        FirMatch::Float32Array {
            values: vec![1.0, 2.0],
            typ: FirType::Array(Box::new(FirType::Float32), 2)
        }
    );
    assert_eq!(
        match_fir(&store, arr_f64),
        FirMatch::Float64Array {
            values: vec![3.5, 4.5],
            typ: FirType::Array(Box::new(FirType::Float64), 2)
        }
    );
    assert_eq!(
        match_fir(&store, arr_q),
        FirMatch::QuadArray {
            values: vec![0.125, 0.25],
            typ: FirType::Array(Box::new(FirType::Quad), 2)
        }
    );
    assert_eq!(
        match_fir(&store, arr_fx),
        FirMatch::FixedPointArray {
            values: vec![0.75, 0.875],
            typ: FirType::Array(Box::new(FirType::FixedPoint), 2)
        }
    );
    assert_eq!(
        match_fir(&store, value_array),
        FirMatch::ValueArray {
            values: vec![q, fx],
            typ: FirType::Array(Box::new(FirType::Float64), 2)
        }
    );
    assert_eq!(
        match_fir(&store, dbi),
        FirMatch::DeclareBufferIterators {
            name1: "in".to_string(),
            name2: "out".to_string(),
            channels: 2,
            typ: FirType::Float32,
            mutable: true,
            chunk: false
        }
    );
    assert_eq!(
        match_fir(&store, ifor),
        FirMatch::IteratorForLoop {
            iterators: vec!["i".to_string(), "j".to_string()],
            is_reverse: true,
            body
        }
    );
    assert_eq!(
        match_fir(&store, sound),
        FirMatch::AddSoundfile {
            label: "sf".to_string(),
            url: "stereo.wav".to_string(),
            var: "fSound0".to_string()
        }
    );
}

#[test]
fn builder_and_match_cover_table_nodes() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let i0 = b.int32(0);
    let v0 = b.float64(1.0);
    let v1 = b.float64(-2.0);
    let table = b.declare_table("fTbl0", AccessType::Struct, FirType::FaustFloat, &[v0, v1]);
    let read = b.load_table("fTbl0", AccessType::Struct, i0, FirType::FaustFloat);
    let write = b.store_table("fTbl0", AccessType::Struct, i0, read);

    assert_eq!(
        match_fir(&store, table),
        FirMatch::DeclareTable {
            name: "fTbl0".to_string(),
            access: AccessType::Struct,
            elem_type: FirType::FaustFloat,
            values: vec![v0, v1]
        }
    );
    assert_eq!(
        match_fir(&store, read),
        FirMatch::LoadTable {
            name: "fTbl0".to_string(),
            access: AccessType::Struct,
            index: i0,
            typ: FirType::FaustFloat
        }
    );
    assert_eq!(
        match_fir(&store, write),
        FirMatch::StoreTable {
            name: "fTbl0".to_string(),
            access: AccessType::Struct,
            index: i0,
            value: read
        }
    );
    assert_eq!(store.value_type(read), Some(FirType::FaustFloat));
}

#[test]
fn builder_and_match_cover_faust_dsp_api_fun_signatures() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);
    let body = b.block(&[]);

    let metadata_args = vec![NamedType {
        name: "m".to_string(),
        typ: FirType::Meta,
    }];
    let metadata_ty = FirType::Fun {
        args: vec![FirType::Meta],
        ret: Box::new(FirType::Void),
    };
    let metadata = b.declare_fun(
        "metadata",
        metadata_ty.clone(),
        &metadata_args,
        Some(body),
        false,
    );

    let ui_args = vec![NamedType {
        name: "ui_interface".to_string(),
        typ: FirType::UI,
    }];
    let ui_ty = FirType::Fun {
        args: vec![FirType::UI],
        ret: Box::new(FirType::Void),
    };
    let build_ui = b.declare_fun(
        "buildUserInterface",
        ui_ty.clone(),
        &ui_args,
        Some(body),
        false,
    );

    let compute_args = vec![
        NamedType {
            name: "count".to_string(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute_ty = FirType::Fun {
        args: vec![
            FirType::Int32,
            FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        ],
        ret: Box::new(FirType::Void),
    };
    let compute = b.declare_fun(
        "compute",
        compute_ty.clone(),
        &compute_args,
        Some(body),
        false,
    );

    assert_eq!(
        match_fir(&store, metadata),
        FirMatch::DeclareFun {
            name: "metadata".to_string(),
            typ: metadata_ty,
            args: metadata_args,
            body: Some(body),
            is_inline: false
        }
    );
    assert_eq!(
        match_fir(&store, build_ui),
        FirMatch::DeclareFun {
            name: "buildUserInterface".to_string(),
            typ: ui_ty,
            args: ui_args,
            body: Some(body),
            is_inline: false
        }
    );
    assert_eq!(
        match_fir(&store, compute),
        FirMatch::DeclareFun {
            name: "compute".to_string(),
            typ: compute_ty,
            args: compute_args,
            body: Some(body),
            is_inline: false
        }
    );
}

#[test]
fn builder_and_match_cover_declare_fun_proto() {
    let mut store = FirStore::new();
    let args = vec![NamedType {
        name: "x".to_string(),
        typ: FirType::FaustFloat,
    }];
    let typ = FirType::Fun {
        args: vec![FirType::FaustFloat],
        ret: Box::new(FirType::FaustFloat),
    };
    let (proto, proto_dup, proto_with_body) = {
        let mut b = FirBuilder::new(&mut store);
        let p = b.declare_fun("myHelper", typ.clone(), &args, None, false);
        let pd = b.declare_fun("myHelper", typ.clone(), &args, None, false);
        let body = b.block(&[]);
        let pb = b.declare_fun("myHelper", typ.clone(), &args, Some(body), false);
        (p, pd, pb)
    };
    // Prototypes are hash-consed.
    assert_eq!(proto, proto_dup);
    // A prototype and a definition with the same signature are distinct nodes.
    assert_ne!(proto, proto_with_body);
    // Round-trip decode.
    assert_eq!(
        match_fir(&store, proto),
        FirMatch::DeclareFun {
            name: "myHelper".to_string(),
            typ,
            args,
            body: None,
            is_inline: false,
        }
    );
}

#[test]
fn structurally_identical_nodes_are_shared() {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let a1 = b.int32(42);
    let a2 = b.int32(42);
    assert_eq!(a1, a2);

    let add1 = b.binop(FirBinOp::Add, a1, a2, FirType::Int32);
    let add2 = b.binop(FirBinOp::Add, a1, a2, FirType::Int32);
    assert_eq!(add1, add2);
}

#[test]
fn match_unknown_on_non_fir_node() {
    let mut store = FirStore::new();
    let raw = store.arena.int(999);
    assert_eq!(match_fir(&store, raw), FirMatch::Unknown);
    assert_eq!(store.value_type(raw), None);
}
