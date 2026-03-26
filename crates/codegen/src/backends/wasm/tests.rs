use super::{WasmMemoryLayout, WasmOptions, generate_wasm_module};
use crate::fixtures::{
    build_control_flow_test_module, build_math_intrinsics_test_module,
    build_passthrough_test_module, build_sine_phasor_test_module,
    build_table_state_delay_test_module,
};

use fir::{AccessType, FirBuilder, FirId, FirMathOp, FirStore, FirType, NamedType};

use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn wasm_scaffold_emits_valid_module_for_passthrough_fixture() {
    let (store, module) = build_passthrough_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit a valid module");

    Validator::new()
        .validate_all(&out.wasm_binary)
        .expect("generated scaffold should validate as WASM");
    assert!(out.dsp_json.contains("\"inputs\":1"));
    assert!(out.dsp_json.contains("\"outputs\":1"));
}

#[test]
fn wasm_scaffold_exports_canonical_faust_api_names() {
    let (store, module) = build_passthrough_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit export section");

    let mut exports = Vec::new();
    for payload in Parser::new(0).parse_all(&out.wasm_binary) {
        let payload = payload.expect("payload should decode");
        if let Payload::ExportSection(section) = payload {
            for export in section {
                let export = export.expect("export should decode");
                exports.push(export.name.to_owned());
            }
        }
    }

    assert_eq!(
        exports,
        vec![
            "compute",
            "getNumInputs",
            "getNumOutputs",
            "getParamValue",
            "getSampleRate",
            "init",
            "instanceClear",
            "instanceConstants",
            "instanceInit",
            "instanceResetUserInterface",
            "setParamValue",
            "memory",
        ]
    );
}

#[test]
fn wasm_compute_passthrough_lowers_loop_and_sample_io() {
    let (store, module) = build_passthrough_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit lowered passthrough compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::Loop { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::BrIf { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::F32Load { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::F32Store { .. })));
}

#[test]
fn wasm_compute_ignores_label_markers() {
    let (store, module) = build_labeled_passthrough_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should lower compute despite FIR labels");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::Loop { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::BrIf { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::F32Load { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::F32Store { .. })));
}

#[test]
fn wasm_compute_lowers_struct_state_and_casts() {
    let (store, module) = build_struct_state_cast_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit struct-state compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::F32Add)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F64PromoteF32)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F64Store { .. })));
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F32Load { .. }))
            .count()
            >= 2
    );
}

#[test]
fn wasm_compute_lowers_struct_tables_and_select2() {
    let (store, module) = build_table_state_delay_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit struct-table compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::Select)));
    assert!(ops.iter().any(|op| matches!(op, Operator::I32GeS)));
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F32Load { .. }))
            .count()
            >= 2
    );
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F32Store { .. }))
            .count()
            >= 2
    );
}

#[test]
fn wasm_compute_lowers_native_math_fun_calls() {
    let (store, module) = build_native_math_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit native math compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::F64Abs)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F64Nearest)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F64Min)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F64Max)));
}

#[test]
fn wasm_compute_lowers_int_abs_inline() {
    let (store, module) = build_int_abs_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit inline int abs");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::I32Sub)));
    assert!(ops.iter().any(|op| matches!(op, Operator::I32LtS)));
    assert!(ops.iter().any(|op| matches!(op, Operator::Select)));
}

#[test]
fn wasm_compute_calls_internal_max_i_helper() {
    let (store, module) = build_internal_max_i_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit internal int helper call");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operator::Call { function_index } if *function_index == 11))
    );
}

#[test]
fn wasm_module_imports_external_math_functions_for_compute() {
    let (store, module) = build_math_intrinsics_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit imported math declarations");

    let mut imports = Vec::new();
    for payload in Parser::new(0).parse_all(&out.wasm_binary) {
        let payload = payload.expect("payload should decode");
        if let Payload::ImportSection(section) = payload {
            for import in section {
                let import = import.expect("import should decode");
                imports.push(import.name.to_owned());
            }
        }
    }

    assert_eq!(imports, vec!["_atan2", "_cos", "_pow", "_sin"]);
}

#[test]
fn wasm_compute_calls_imported_math_functions() {
    let (store, module) = build_math_intrinsics_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit imported math compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Operator::Call { function_index } if *function_index < 4))
            .count()
            >= 4
    );
}

#[test]
fn wasm_compute_lowers_control_flow_statements() {
    let (store, module) = build_control_flow_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit control-flow compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Operator::If { .. }))
            .count()
            >= 2
    );
    assert!(ops.iter().any(|op| matches!(op, Operator::Else)));
    assert!(ops.iter().any(|op| matches!(op, Operator::Drop)));
    assert!(ops.iter().any(|op| matches!(op, Operator::I32Eq)));
}

#[test]
fn wasm_compute_lowers_for_and_while_loops() {
    let (store, module) = build_loop_statement_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit for/while compute body");

    let body = code_body_at(&out.wasm_binary, 1);
    let ops = decode_ops(body);
    assert!(
        ops.iter()
            .filter(|op| matches!(op, Operator::Loop { .. }))
            .count()
            >= 2
    );
    assert!(ops.iter().any(|op| matches!(op, Operator::I32Eqz)));
    assert!(ops.iter().any(|op| matches!(op, Operator::I32GeS)));
    assert!(ops.iter().any(|op| matches!(op, Operator::I32Add)));
}

#[test]
fn wasm_get_sample_rate_loads_struct_field_when_present() {
    let (store, module) = build_sample_rate_state_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit getSampleRate body");

    let body = code_body_at(&out.wasm_binary, 5);
    let ops = decode_ops(body);
    assert!(matches!(ops.as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::I32Load { memarg },
            Operator::End
        ] if memarg.offset == 0
    ));
}

#[test]
fn wasm_get_param_value_loads_real_from_dsp_plus_index() {
    let (store, module) = build_passthrough_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit getParamValue body");

    let body = code_body_at(&out.wasm_binary, 4);
    let ops = decode_ops(body);
    assert!(matches!(ops.as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Add,
            Operator::F32Load { memarg },
            Operator::End
        ] if memarg.offset == 0
    ));
}

#[test]
fn wasm_set_param_value_stores_real_to_dsp_plus_index() {
    let (store, module) = build_passthrough_test_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit setParamValue body");

    let body = code_body_at(&out.wasm_binary, 13);
    let ops = decode_ops(body);
    assert!(matches!(ops.as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Add,
            Operator::LocalGet { local_index: 2 },
            Operator::F32Store { memarg },
            Operator::End
        ] if memarg.offset == 0
    ));
}

#[test]
fn wasm_instance_constants_stores_sample_rate_when_field_exists() {
    let (store, module) = build_sample_rate_state_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit instanceConstants body");

    let body = code_body_at(&out.wasm_binary, 8);
    let ops = decode_ops(body);
    assert!(matches!(ops.as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32Store { memarg },
            Operator::End
        ] if memarg.offset == 0
    ));
}

#[test]
fn wasm_instance_clear_lowers_struct_state_reset_body() {
    let (store, module) = build_instance_clear_state_module();
    let out = generate_wasm_module(&store, module, &WasmOptions::default())
        .expect("WASM scaffold should emit instanceClear body");

    let body = code_body_at(&out.wasm_binary, 7);
    let ops = decode_ops(body);
    assert!(ops.iter().any(|op| matches!(op, Operator::Loop { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::BrIf { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::I32Store { .. })));
    assert!(ops.iter().any(|op| matches!(op, Operator::F32Store { .. })));
}

#[test]
fn wasm_layout_tracks_struct_offsets_for_sine_fixture() {
    let (store, module) = build_sine_phasor_test_module();
    let layout = WasmMemoryLayout::from_module(&store, module, &WasmOptions::default(), 64)
        .expect("sine fixture layout should compute");

    assert_eq!(layout.struct_size, 16);
    assert_eq!(layout.tables_offset, 16);
    assert_eq!(layout.io_zone_offset, 16);
    assert_eq!(layout.field_offsets["fFreq"].offset, 0);
    assert_eq!(layout.field_offsets["fFreq"].size, 4);
    assert_eq!(layout.field_offsets["fGain"].offset, 4);
    assert_eq!(layout.field_offsets["fPhase"].offset, 8);
    assert_eq!(layout.field_offsets["fPhase"].size, 8);
    assert_eq!(layout.pages, 1);
}

#[test]
fn wasm_layout_pads_i32_fields_to_audio_slot_in_double_mode() {
    let (store, module) = build_single_i32_state_module();
    let layout = WasmMemoryLayout::from_module(
        &store,
        module,
        &WasmOptions {
            double_precision: true,
            ..WasmOptions::default()
        },
        32,
    )
    .expect("double-precision layout should compute");

    assert_eq!(layout.struct_size, 8);
    assert_eq!(layout.field_offsets["fMode"].offset, 0);
    assert_eq!(layout.field_offsets["fMode"].size, 8);
}

#[test]
fn wasm_layout_places_static_tables_after_struct_region() {
    let (store, module) = build_static_table_layout_module();
    let layout = WasmMemoryLayout::from_module(&store, module, &WasmOptions::default(), 32)
        .expect("layout with static table should compute");

    assert_eq!(layout.struct_size, 4);
    assert_eq!(layout.tables_offset, 4);
    assert_eq!(layout.field_offsets["fGain"].offset, 0);
    assert_eq!(layout.field_offsets["wav"].offset, 4);
    assert_eq!(layout.field_offsets["wav"].size, 12);
    assert_eq!(layout.io_zone_offset, 16);
}

fn build_sample_rate_state_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let zero = b.int32(0);
    let sample_rate = b.declare_var(
        "fSampleRate",
        FirType::Int32,
        AccessType::Struct,
        Some(zero),
    );
    let globals = b.block(&[sample_rate]);
    let dsp_struct = b.block(&[]);
    let compute = declare_trivial_compute(&mut b);
    let functions = b.block(&[compute]);
    let static_decls = b.block(&[]);
    let module = b.module(0, 1, "sr_dsp", dsp_struct, globals, functions, static_decls);
    (store, module)
}

fn build_labeled_passthrough_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);

    let chan0 = b.int32(0);
    let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let in_ptr = b.load_table("inputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let in_alias = b.declare_var("input0", ptr_ty.clone(), AccessType::Stack, Some(in_ptr));
    let out_alias = b.declare_var("output0", ptr_ty.clone(), AccessType::Stack, Some(out_ptr));

    let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let x = b.load_table("input0", AccessType::Stack, i0, FirType::FaustFloat);
    let write = b.store_table("output0", AccessType::Stack, i0, x);
    let loop_body = b.block(&[write]);
    let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
    let label = b.label("signal_fir_fastlane_step2a: executable base slice");
    let compute_body = b.block(&[label, in_alias, out_alias, sample_loop]);
    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(compute_body),
        false,
    );

    let functions = b.block(&[compute]);
    let static_decls = b.block(&[]);
    let module = b.module(
        1,
        1,
        "labeled_passthrough",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_instance_clear_state_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let zero_i = b.int32(0);
    let scalar = b.declare_var("fIOTA", FirType::Int32, AccessType::Struct, Some(zero_i));
    let delay = b.declare_var(
        "fDelay",
        FirType::Array(Box::new(FirType::FaustFloat), 4),
        AccessType::Struct,
        None,
    );
    let dsp_struct = b.block(&[scalar, delay]);
    let globals = b.block(&[]);
    let static_decls = b.block(&[]);

    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let zero_f = b.float32(0.0);
    let clear_scalar = b.store_var("fIOTA", AccessType::Struct, zero_i);
    let clear_delay = b.store_table("fDelay", AccessType::Struct, i0, zero_f);
    let clear_body = b.block(&[clear_delay]);
    let four = b.int32(4);
    let clear_loop = b.simple_for_loop("i0", four, clear_body, false);
    let instance_clear_body = b.block(&[clear_scalar, clear_loop]);

    let clear_args = [NamedType {
        name: "dsp".to_owned(),
        typ: FirType::Ptr(Box::new(FirType::Obj)),
    }];
    let instance_clear = b.declare_fun(
        "instanceClear",
        FirType::Fun {
            args: vec![FirType::Ptr(Box::new(FirType::Obj))],
            ret: Box::new(FirType::Void),
        },
        &clear_args,
        Some(instance_clear_body),
        false,
    );

    let compute = declare_trivial_compute(&mut b);
    let functions = b.block(&[compute, instance_clear]);
    let module = b.module(
        0,
        0,
        "instance_clear_state",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_struct_state_cast_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let bias_init = b.float32(0.5);
    let level_init = b.float64(0.0);
    let bias = b.declare_var(
        "fBias",
        FirType::FaustFloat,
        AccessType::Struct,
        Some(bias_init),
    );
    let level = b.declare_var(
        "fLevel",
        FirType::Float64,
        AccessType::Struct,
        Some(level_init),
    );
    let globals = b.block(&[bias, level]);
    let dsp_struct = b.block(&[]);

    let chan0 = b.int32(0);
    let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let in_ptr = b.load_table("inputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let in_alias = b.declare_var("input0", ptr_ty.clone(), AccessType::Stack, Some(in_ptr));
    let out_alias = b.declare_var("output0", ptr_ty, AccessType::Stack, Some(out_ptr));

    let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let x = b.load_table("input0", AccessType::Stack, i0, FirType::FaustFloat);
    let bias_cur = b.load_var("fBias", AccessType::Struct, FirType::FaustFloat);
    let y = b.binop(fir::FirBinOp::Add, x, bias_cur, FirType::FaustFloat);
    let y_f64 = b.cast(FirType::Float64, y);
    let store_level = b.store_var("fLevel", AccessType::Struct, y_f64);
    let store_bias = b.store_var("fBias", AccessType::Struct, y);
    let store_out = b.store_table("output0", AccessType::Stack, i0, y);
    let loop_body = b.block(&[store_level, store_bias, store_out]);
    let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
    let compute_body = b.block(&[in_alias, out_alias, sample_loop]);

    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(compute_body),
        false,
    );
    let functions = b.block(&[compute]);
    let static_decls = b.block(&[]);
    let module = b.module(
        1,
        1,
        "state_cast_dsp",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_native_math_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);

    let chan0 = b.int32(0);
    let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let in_ptr = b.load_table("inputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let in_alias = b.declare_var("input0", ptr_ty.clone(), AccessType::Stack, Some(in_ptr));
    let out_alias = b.declare_var("output0", ptr_ty, AccessType::Stack, Some(out_ptr));

    let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let x = b.load_table("input0", AccessType::Stack, i0, FirType::FaustFloat);
    let x_f64 = b.cast(FirType::Float64, x);
    let absx = b.math_call(FirMathOp::Abs, &[x_f64], FirType::Float64);
    let one = b.float64(1.0);
    let zero = b.float64(0.0);
    let minv = b.math_call(FirMathOp::Min, &[absx, one], FirType::Float64);
    let maxv = b.math_call(FirMathOp::Max, &[minv, zero], FirType::Float64);
    let rounded = b.math_call(FirMathOp::Rint, &[maxv], FirType::Float64);
    let y = b.cast(FirType::FaustFloat, rounded);
    let store_out = b.store_table("output0", AccessType::Stack, i0, y);
    let loop_body = b.block(&[store_out]);
    let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
    let compute_body = b.block(&[in_alias, out_alias, sample_loop]);

    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(compute_body),
        false,
    );
    let functions = b.block(&[compute]);
    let static_decls = b.block(&[]);
    let module = b.module(
        1,
        1,
        "native_math_dsp",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_internal_max_i_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);
    let static_decls = b.block(&[]);

    let chan0 = b.int32(0);
    let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let out_alias = b.declare_var("output0", ptr_ty, AccessType::Stack, Some(out_ptr));
    let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let clamped = b.fun_call("max_i", &[i0, count], FirType::Int32);
    let clamped_f32 = b.cast(FirType::FaustFloat, clamped);
    let write = b.store_table("output0", AccessType::Stack, i0, clamped_f32);
    let loop_body = b.block(&[write]);
    let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
    let compute_body = b.block(&[out_alias, sample_loop]);
    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(compute_body),
        false,
    );
    let functions = b.block(&[compute]);
    let module = b.module(
        0,
        1,
        "internal_max_i",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_int_abs_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);
    let static_decls = b.block(&[]);

    let chan0 = b.int32(0);
    let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let out_alias = b.declare_var("output0", ptr_ty, AccessType::Stack, Some(out_ptr));
    let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let neg_i0 = b.neg(i0, FirType::Int32);
    let abs_i0 = b.fun_call("abs", &[neg_i0], FirType::Int32);
    let abs_f32 = b.cast(FirType::FaustFloat, abs_i0);
    let write = b.store_table("output0", AccessType::Stack, i0, abs_f32);
    let loop_body = b.block(&[write]);
    let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
    let compute_body = b.block(&[out_alias, sample_loop]);
    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(compute_body),
        false,
    );
    let functions = b.block(&[compute]);
    let module = b.module(
        0,
        1,
        "int_abs",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_loop_statement_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);

    let chan0 = b.int32(0);
    let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
    let out_alias = b.declare_var("output0", ptr_ty, AccessType::Stack, Some(out_ptr));

    let acc_zero = b.int32(0);
    let acc_decl = b.declare_var("acc", FirType::Int32, AccessType::Stack, Some(acc_zero));

    let false_cond = b.bool_(false);
    let drop_zero = b.drop_(acc_zero);
    let while_body = b.block(&[drop_zero]);
    let while_loop = b.while_loop(false_cond, while_body);

    let init = b.int32(0);
    let end = b.int32(4);
    let step = b.int32(1);
    let i = b.load_var("i", AccessType::Loop, FirType::Int32);
    let acc_cur = b.load_var("acc", AccessType::Stack, FirType::Int32);
    let acc_next = b.binop(fir::FirBinOp::Add, acc_cur, i, FirType::Int32);
    let store_acc = b.store_var("acc", AccessType::Stack, acc_next);
    let i_f32 = b.cast(FirType::FaustFloat, i);
    let store_out = b.store_table("output0", AccessType::Stack, i, i_f32);
    let for_body = b.block(&[store_acc, store_out]);
    let for_loop = b.for_loop("i", init, end, step, for_body, false);

    let compute_body = b.block(&[out_alias, acc_decl, while_loop, for_loop]);
    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(compute_body),
        false,
    );
    let functions = b.block(&[compute]);
    let static_decls = b.block(&[]);
    let module = b.module(
        0,
        1,
        "loop_stmt_dsp",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_single_i32_state_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let zero = b.int32(0);
    let mode = b.declare_var("fMode", FirType::Int32, AccessType::Struct, Some(zero));
    let globals = b.block(&[mode]);
    let dsp_struct = b.block(&[]);
    let compute = declare_trivial_compute(&mut b);
    let functions = b.block(&[compute]);
    let static_decls = b.block(&[]);
    let module = b.module(
        0,
        1,
        "mode_dsp",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn build_static_table_layout_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let gain_init = b.float32(0.0);
    let gain = b.declare_var(
        "fGain",
        FirType::FaustFloat,
        AccessType::Struct,
        Some(gain_init),
    );
    let globals = b.block(&[gain]);
    let w0 = b.float32(0.0);
    let w1 = b.float32(1.0);
    let w2 = b.float32(2.0);
    let wav = b.declare_table(
        "wav",
        AccessType::Static,
        FirType::FaustFloat,
        &[w0, w1, w2],
    );
    let dsp_struct = b.block(&[]);
    let compute = declare_trivial_compute(&mut b);
    let functions = b.block(&[compute]);
    let static_decls = b.block(&[wav]);
    let module = b.module(
        0,
        1,
        "table_dsp",
        dsp_struct,
        globals,
        functions,
        static_decls,
    );
    (store, module)
}

fn declare_trivial_compute(b: &mut FirBuilder<'_>) -> FirId {
    let args = [
        NamedType {
            name: "dsp".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_owned(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_owned(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let body = b.block(&[]);
    b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &args,
        Some(body),
        false,
    )
}

fn code_body_at<'a>(wasm: &'a [u8], index: usize) -> wasmparser::FunctionBody<'a> {
    let mut current = 0usize;
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload.expect("payload should decode");
        if let Payload::CodeSectionEntry(body) = payload {
            if current == index {
                return body;
            }
            current += 1;
        }
    }
    panic!("code body index {index} not found");
}

fn decode_ops(body: wasmparser::FunctionBody<'_>) -> Vec<Operator<'_>> {
    body.get_operators_reader()
        .expect("operators reader")
        .into_iter()
        .map(|op| op.expect("operator should decode"))
        .collect()
}
