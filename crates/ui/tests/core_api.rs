//! Integration tests for `ui` canonical builder/matcher APIs.

use tlib::TreeArena;
use ui::{
    ControlKind, ControlRange, ControlSpec, UiBuilder, UiGroupKind, UiMatch, UiProgram, match_ui,
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
            children: vec![input, output, soundfile],
        }
    );
    assert_eq!(
        match_ui(&arena, hgroup),
        UiMatch::Group {
            kind: UiGroupKind::Horizontal,
            label: "row",
            children: vec![input],
        }
    );
    assert_eq!(
        match_ui(&arena, tgroup),
        UiMatch::Group {
            kind: UiGroupKind::Tab,
            label: "tabs",
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
    };

    assert_eq!(program.controls.len(), 1);
    assert_eq!(program.controls[0].kind, ControlKind::Checkbox);
    assert_eq!(program.controls[0].label, "c");
    assert_eq!(
        match_ui(&program.arena, program.root),
        UiMatch::Group {
            kind: UiGroupKind::Horizontal,
            label: "top",
            children: vec![checkbox],
        }
    );
}
