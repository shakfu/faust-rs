//! Shared helpers for the `signal_prepare` test groups.

use signals::{SigMatch, match_sig};

pub(super) fn subtree_contains_fconst(arena: &tlib::TreeArena, sig: signals::SigId) -> bool {
    if matches!(match_sig(arena, sig), SigMatch::FConst(_, _, _)) {
        return true;
    }
    let Some(node) = arena.node(sig) else {
        return false;
    };
    node.children
        .as_slice()
        .iter()
        .copied()
        .any(|child| subtree_contains_fconst(arena, child))
}
