use super::*;

/// Evaluates one UI/modulation label node using the C++ `evalLabel(...)`
/// placeholder semantics.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `evalLabel(...)`
/// - `writeIdentValue(...)`
///
/// Mapping status: `adapted`.
/// Rust mirrors the C++ label substitution state machine while resolving
/// placeholder values through explicit evaluator helpers instead of global tree
/// properties.
pub(crate) fn eval_label_node(
    arena: &mut TreeArena,
    label_node: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<String, EvalError> {
    let Some(src) = label_node_text(arena, label_node) else {
        return Err(EvalError::InvalidModulationLabel { node: label_node });
    };
    let src = src.to_owned();
    eval_label(arena, &src, env, loop_detector)
}

/// Port of the C++ `evalLabel(...)` mini-parser used for dynamic UI labels.
pub(crate) fn eval_label(
    arena: &mut TreeArena,
    src: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<String, EvalError> {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        AfterPercent,
        Ident,
        BracedIdent,
    }

    let chars: Vec<char> = src.chars().collect();
    let mut idx = 0usize;
    let mut state = State::Text;
    let mut dst = String::new();
    let mut ident = String::new();
    let mut format = String::new();

    while idx <= chars.len() {
        let cur = chars.get(idx).copied();
        match state {
            State::Text => match cur {
                None => break,
                Some('%') => {
                    ident.clear();
                    format.clear();
                    state = State::AfterPercent;
                    idx += 1;
                }
                Some(ch) => {
                    dst.push(ch);
                    idx += 1;
                }
            },
            State::AfterPercent => match cur {
                None => {
                    dst.push('%');
                    dst.push_str(&format);
                    break;
                }
                Some(ch) if ch.is_ascii_digit() => {
                    format.push(ch);
                    idx += 1;
                }
                Some(ch) if is_eval_label_ident_char(ch) => {
                    ident.push(ch);
                    state = State::Ident;
                    idx += 1;
                }
                Some('{') => {
                    state = State::BracedIdent;
                    idx += 1;
                }
                Some(_) => {
                    dst.push('%');
                    dst.push_str(&format);
                    state = State::Text;
                }
            },
            State::Ident => match cur {
                Some(ch) if is_eval_label_ident_char(ch) => {
                    ident.push(ch);
                    idx += 1;
                }
                _ => {
                    write_label_ident_value(arena, &mut dst, &format, &ident, env, loop_detector)?;
                    state = State::Text;
                }
            },
            State::BracedIdent => match cur {
                Some(ch) if is_eval_label_ident_char(ch) => {
                    ident.push(ch);
                    idx += 1;
                }
                Some('}') => {
                    write_label_ident_value(arena, &mut dst, &format, &ident, env, loop_detector)?;
                    idx += 1;
                    state = State::Text;
                }
                _ => {
                    dst.push('%');
                    dst.push_str(&format);
                    break;
                }
            },
        }
    }

    Ok(dst)
}

/// Returns `true` for identifier characters accepted by `%ident` label syntax.
///
/// This intentionally follows the conservative subset used by the current Rust
/// port of `evalLabel(...)`: ASCII alphanumerics plus `_`.
pub(crate) fn is_eval_label_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Renders one `%ident` or `%{ident}` placeholder into the destination label.
///
/// Width formatting follows the C++ `evalLabel(...)` convention implemented by
/// the active corpus: the optional decimal field width is clamped to `0..=4`
/// before rendering the resolved integer value.
pub(crate) fn write_label_ident_value(
    arena: &mut TreeArena,
    dst: &mut String,
    format: &str,
    ident: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<(), EvalError> {
    let width = format.parse::<usize>().unwrap_or(0).clamp(0, 4);
    let value = eval_ident_to_constant_int(arena, ident, env, loop_detector)?;
    let rendered = if width == 0 {
        value.to_string()
    } else {
        format!("{value:>width$}")
    };
    dst.push_str(&rendered);
    Ok(())
}

/// Extracts the plain-text label content from one label node.
///
/// Missing/invalid label nodes degrade to an empty string so modulation path
/// reconstruction stays total during recursive traversal.
pub(crate) fn strip_label_node(arena: &TreeArena, label: TreeId) -> String {
    label_node_text(arena, label)
        .map(strip_label_metadata)
        .unwrap_or_default()
        .to_owned()
}

/// Removes Faust metadata suffixes from one textual label.
///
/// For example `gain [unit:dB]` becomes `gain`. The returned slice borrows from
/// the original string and is intended for path matching, not for user-facing
/// pretty-printing.
pub(crate) fn strip_label_metadata(label: &str) -> &str {
    label
        .split_once('[')
        .map_or(label, |(prefix, _)| prefix)
        .trim()
}

/// Returns the raw textual payload of a label node, if any.
///
/// Both string literals and interned symbols are accepted to stay compatible
/// with transitional tree encodings.
pub(crate) fn label_node_text(arena: &TreeArena, label: TreeId) -> Option<&str> {
    match arena.kind(label) {
        Some(NodeKind::StringLiteral(label)) => Some(label.as_ref()),
        Some(NodeKind::Symbol(label)) => Some(label.as_ref()),
        _ => None,
    }
}

/// Returns `true` when `needle` appears in-order inside `haystack`.
///
/// This relaxed path relation is used by the current modulation implementation
/// so target paths can match inside nested UI groups without requiring exact
/// absolute-path equality.
pub(crate) fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut haystack_iter = haystack.iter();
    needle
        .iter()
        .all(|target| haystack_iter.by_ref().any(|candidate| candidate == target))
}
