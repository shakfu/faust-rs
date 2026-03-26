use super::{WasmOptions, generate_wasm_module};
use crate::fixtures::build_passthrough_test_module;

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
