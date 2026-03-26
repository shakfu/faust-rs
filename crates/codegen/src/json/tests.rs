use super::{JsonBuildOptions, JsonDescription, build_json_description_from_fir};
use crate::fixtures::{build_gain_bias_ui_meta_test_module, build_passthrough_test_module};

use fir::{FirMatch, match_fir};

#[test]
fn json_description_renders_minimal_shape() {
    let json = JsonDescription {
        name: "passthrough".to_owned(),
        filename: None,
        version: None,
        compile_options: None,
        library_list: Vec::new(),
        include_pathnames: Vec::new(),
        size: Some(4),
        inputs: 1,
        outputs: 2,
        sr_index: None,
        meta: Vec::new(),
        ui: Vec::new(),
    }
    .render();

    assert_eq!(
        json,
        "{\"name\":\"passthrough\",\"size\":4,\"inputs\":1,\"outputs\":2,\"ui\":[]}"
    );
}

#[test]
fn json_description_escapes_strings() {
    let json = JsonDescription {
        name: "quote\"slash\\tab\tline\n".to_owned(),
        filename: None,
        version: None,
        compile_options: None,
        library_list: Vec::new(),
        include_pathnames: Vec::new(),
        size: Some(0),
        inputs: 0,
        outputs: 0,
        sr_index: None,
        meta: Vec::new(),
        ui: Vec::new(),
    }
    .render();

    assert!(json.contains("\"name\":\"quote\\\"slash\\\\tab\\tline\\n\""));
}

#[test]
fn json_builder_replays_fir_ui_and_metadata() {
    let (store, module) = build_gain_bias_ui_meta_test_module();
    let FirMatch::Module {
        functions,
        num_inputs,
        num_outputs,
        ..
    } = match_fir(&store, module)
    else {
        panic!("module root expected");
    };
    let FirMatch::Block(function_items) = match_fir(&store, functions) else {
        panic!("function block expected");
    };

    let json = build_json_description_from_fir(
        &store,
        &function_items,
        JsonBuildOptions {
            name: "gain_bias_ui_meta".to_owned(),
            filename: None,
            version: None,
            compile_options: None,
            library_list: Vec::new(),
            include_pathnames: Vec::new(),
            size: Some(16),
            inputs: num_inputs,
            outputs: num_outputs,
            sr_index: Some(0),
        },
        |var| match var {
            "fGain" => Some(0),
            "fBias" => Some(4),
            "fGate" => Some(8),
            "fLevel" => Some(12),
            _ => None,
        },
    )
    .expect("FIR JSON builder should succeed");

    let rendered = json.render();
    assert!(
        rendered.contains("\"meta\":[{\"name\":\"gain-bias-ui-meta\"},{\"author\":\"faust-rs\"}]")
    );
    assert!(rendered.contains("\"address\":\"/GainBias/gain\""));
    assert!(rendered.contains("\"index\":0"));
    assert!(rendered.contains("\"address\":\"/GainBias/level\""));
    assert!(rendered.contains("\"index\":12"));
}

#[test]
fn json_builder_emits_empty_ui_when_build_ui_function_is_missing() {
    let (store, module) = build_passthrough_test_module();
    let FirMatch::Module {
        functions,
        num_inputs,
        num_outputs,
        ..
    } = match_fir(&store, module)
    else {
        panic!("module root expected");
    };
    let FirMatch::Block(function_items) = match_fir(&store, functions) else {
        panic!("function block expected");
    };

    let json = build_json_description_from_fir(
        &store,
        &function_items,
        JsonBuildOptions {
            name: "passthrough".to_owned(),
            filename: None,
            version: None,
            compile_options: None,
            library_list: Vec::new(),
            include_pathnames: Vec::new(),
            size: Some(0),
            inputs: num_inputs,
            outputs: num_outputs,
            sr_index: None,
        },
        |_var| None,
    )
    .expect("builder should tolerate missing buildUserInterface");

    assert_eq!(json.ui, Vec::new());
}
