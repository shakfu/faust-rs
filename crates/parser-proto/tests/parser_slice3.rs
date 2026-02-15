use std::fs;
use std::path::Path;

use boxes::{dump_box, is_box_hslider, is_box_iprod, is_box_iseq, is_box_isum};
use parser_proto::parse_program;
use tlib::{TreeArena, TreeId};

fn list_head(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.hd(list).expect("list must be non-empty")
}

fn list_tail(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.tl(list).expect("list tail must exist")
}

fn definition_expr(arena: &TreeArena, def: TreeId) -> TreeId {
    let payload = list_tail(arena, def);
    list_tail(arena, payload)
}

#[test]
fn supports_ui_slider_constructor() {
    let output = parse_program(
        "process = hslider(\"gain\", 0.5, 0.0, 1.0, 0.01);",
        "slice3_ui.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    assert!(is_box_hslider(&output.state.arena, expr).is_some());
}

#[test]
fn supports_iterative_seq_sum_prod() {
    for (src, name, pred) in [
        (
            "process = seq(i, 4, _);",
            "iseq",
            is_box_iseq as fn(&TreeArena, TreeId) -> Option<(TreeId, TreeId, TreeId)>,
        ),
        (
            "process = sum(i, 4, _);",
            "isum",
            is_box_isum as fn(&TreeArena, TreeId) -> Option<(TreeId, TreeId, TreeId)>,
        ),
        (
            "process = prod(i, 4, _);",
            "iprod",
            is_box_iprod as fn(&TreeArena, TreeId) -> Option<(TreeId, TreeId, TreeId)>,
        ),
    ] {
        let output = parse_program(src, "slice3_iter.dsp");
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors for {name}: {:?}",
            output.errors
        );
        let root = output.root.expect("root should be present");
        let def = list_head(&output.state.arena, root);
        let expr = definition_expr(&output.state.arena, def);
        assert!(
            pred(&output.state.arena, expr).is_some(),
            "{name} should parse"
        );
    }
}

#[test]
fn supports_recursion_form_plus_tilde_wire() {
    let output = parse_program("process = + ~ _;", "slice3_rec.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    assert_eq!(
        dump_box(&output.state.arena, expr),
        "BOXREC(BOXADD(), BOXWIRE())"
    );
}

#[test]
fn parse_only_rep_corpus_subset_is_accepted() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let mut files = fs::read_dir(&corpus)
        .expect("corpus directory should be readable")
        .map(|entry| entry.expect("corpus entry should be readable").path())
        .filter(|path| {
            let is_dsp = path.extension().is_some_and(|ext| ext == "dsp");
            let is_rep = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rep_"));
            is_dsp && is_rep
        })
        .collect::<Vec<_>>();
    files.sort();
    assert!(
        !files.is_empty(),
        "corpus rep_*.dsp list should not be empty"
    );

    for path in files {
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("corpus file name should be valid utf-8");
        let source = fs::read_to_string(&path).expect("corpus file should be readable");
        let output = parse_program(&source, file);
        assert!(
            output.errors.is_empty(),
            "{file} should parse without parser errors, got {:?}",
            output.errors
        );
        assert!(output.root.is_some(), "{file} should produce a root");
    }
}
