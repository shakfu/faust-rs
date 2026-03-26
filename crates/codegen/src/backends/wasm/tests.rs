use super::{WasmMemoryLayout, WasmOptions, generate_wasm_module};
use crate::fixtures::{build_passthrough_test_module, build_sine_phasor_test_module};

use fir::{AccessType, FirBuilder, FirId, FirStore, FirType, NamedType};

use wasmparser::{Parser, Payload, Validator};

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
