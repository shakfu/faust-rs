//! Integration tests for `ui` canonical builder/matcher APIs.

use tlib::TreeArena;
use ui::{
    ControlKind, ControlRange, ControlSpec, UiBuilder, UiGroupKind, UiMatch, UiProgram,
    UiRootOrigin, match_ui, split_label_metadata,
};

#[test]
fn builder_and_match_cover_grouped_ui_shapes() {
    let mut arena = TreeArena::new();
    let mut b = UiBuilder::new(&mut arena);

    let input = b.input_control(7);
    let output = b.output_control(8);
    let soundfile = b.soundfile(9);
    let vgroup = b.vgroup("root", &[input, output, soundfile]);
    let hgroup = b.hgroup("row", &[input]);
    let tgroup = b.tgroup("tabs", &[output]);

    assert_eq!(match_ui(&arena, input), UiMatch::InputControl(7));
    assert_eq!(match_ui(&arena, output), UiMatch::OutputControl(8));
    assert_eq!(match_ui(&arena, soundfile), UiMatch::Soundfile(9));
    assert_eq!(
        match_ui(&arena, vgroup),
        UiMatch::Group {
            kind: UiGroupKind::Vertical,
            label: "root",
            metadata: vec![],
            children: vec![input, output, soundfile],
        }
    );
    assert_eq!(
        match_ui(&arena, hgroup),
        UiMatch::Group {
            kind: UiGroupKind::Horizontal,
            label: "row",
            metadata: vec![],
            children: vec![input],
        }
    );
    assert_eq!(
        match_ui(&arena, tgroup),
        UiMatch::Group {
            kind: UiGroupKind::Tab,
            label: "tabs",
            metadata: vec![],
            children: vec![output],
        }
    );
}

#[test]
fn group_children_preserve_source_order() {
    let mut arena = TreeArena::new();
    let mut b = UiBuilder::new(&mut arena);

    let c0 = b.input_control(1);
    let c1 = b.input_control(2);
    let c2 = b.output_control(3);
    let root = b.hgroup("ordered", &[c0, c1, c2]);

    let UiMatch::Group { children, .. } = match_ui(&arena, root) else {
        panic!("group expected");
    };
    assert_eq!(children, vec![c0, c1, c2]);
}

#[test]
fn group_metadata_round_trips_through_builder_and_matcher() {
    let mut arena = TreeArena::new();
    let mut b = UiBuilder::new(&mut arena);

    let control = b.input_control(1);
    let root = b.group_with_metadata(
        UiGroupKind::Horizontal,
        "top",
        &[
            ("style".to_owned(), "tabbed".to_owned()),
            ("tooltip".to_owned(), "hello".to_owned()),
        ],
        &[control],
    );

    assert_eq!(
        match_ui(&arena, root),
        UiMatch::Group {
            kind: UiGroupKind::Horizontal,
            label: "top",
            metadata: vec![
                ("style".to_owned(), "tabbed".to_owned()),
                ("tooltip".to_owned(), "hello".to_owned()),
            ],
            children: vec![control],
        }
    );
}

#[test]
fn ui_program_keeps_root_and_control_registry() {
    let mut arena = TreeArena::new();
    let mut b = UiBuilder::new(&mut arena);
    let checkbox = b.input_control(0);
    let root = b.hgroup("top", &[checkbox]);

    let controls = vec![ControlSpec {
        id: 0,
        kind: ControlKind::Checkbox,
        label: "c".to_owned(),
        metadata: vec![("style".to_owned(), "knob".to_owned())],
        range: Some(ControlRange {
            init: 0.0,
            min: 0.0,
            max: 1.0,
            step: 1.0,
        }),
    }];
    let program = UiProgram {
        arena,
        root,
        controls,
        root_origin: UiRootOrigin::Explicit,
        emit_ui: true,
    };

    assert_eq!(program.controls.len(), 1);
    assert_eq!(program.controls[0].kind, ControlKind::Checkbox);
    assert_eq!(program.controls[0].label, "c");
    assert_eq!(
        match_ui(&program.arena, program.root),
        UiMatch::Group {
            kind: UiGroupKind::Horizontal,
            label: "top",
            metadata: vec![],
            children: vec![checkbox],
        }
    );
    assert_eq!(
        program.control(0).map(|control| control.label.as_str()),
        Some("c")
    );
}

#[test]
fn ui_program_empty_is_non_emitting_placeholder_with_empty_vertical_root() {
    let program = UiProgram::empty();

    assert!(program.is_empty());
    assert!(program.controls.is_empty());
    assert_eq!(program.root_origin, UiRootOrigin::Synthesized);
    assert_eq!(
        match_ui(&program.arena, program.root),
        UiMatch::Group {
            kind: UiGroupKind::Vertical,
            label: "",
            metadata: vec![],
            children: vec![],
        }
    );
}

#[test]
fn control_lookup_uses_stable_ids_across_input_output_and_soundfile_controls() {
    let mut arena = TreeArena::new();
    let mut b = UiBuilder::new(&mut arena);
    let checkbox = b.input_control(0);
    let meter = b.output_control(1);
    let soundfile = b.soundfile(2);
    let root = b.vgroup("root", &[checkbox, meter, soundfile]);

    let program = UiProgram {
        arena,
        root,
        controls: vec![
            ControlSpec {
                id: 0,
                kind: ControlKind::Checkbox,
                label: "gate".to_owned(),
                metadata: Vec::new(),
                range: None,
            },
            ControlSpec {
                id: 1,
                kind: ControlKind::HBargraph,
                label: "level".to_owned(),
                metadata: Vec::new(),
                range: Some(ControlRange {
                    init: 0.0,
                    min: -60.0,
                    max: 6.0,
                    step: 0.0,
                }),
            },
            ControlSpec {
                id: 2,
                kind: ControlKind::Soundfile,
                label: "sample".to_owned(),
                metadata: vec![("url".to_owned(), "{'tests/assets/silence.wav'}".to_owned())],
                range: None,
            },
        ],
        root_origin: UiRootOrigin::Explicit,
        emit_ui: true,
    };

    assert_eq!(
        program.control(0).map(|control| control.kind),
        Some(ControlKind::Checkbox)
    );
    assert_eq!(
        program.control(1).map(|control| control.kind),
        Some(ControlKind::HBargraph)
    );
    assert_eq!(
        program.control(2).map(|control| control.kind),
        Some(ControlKind::Soundfile)
    );
    assert!(program.control(3).is_none());
}

#[test]
fn split_label_metadata_matches_cpp_style_widget_metadata_extraction() {
    let (label, metadata) = split_label_metadata("gain [style:knob]");

    assert_eq!(label, "gain");
    assert_eq!(metadata, vec![("style".to_owned(), "knob".to_owned())]);
}

#[test]
fn split_label_metadata_handles_nested_brackets_and_escapes() {
    let (label, metadata) =
        split_label_metadata(r#"tab \[main\] [tooltip: use [fast\:slow]] [unit: dB]"#);

    assert_eq!(label, "tab [main]");
    assert_eq!(
        metadata,
        vec![
            ("tooltip".to_owned(), "use [fast:slow]".to_owned()),
            ("unit".to_owned(), "dB".to_owned()),
        ]
    );
}
