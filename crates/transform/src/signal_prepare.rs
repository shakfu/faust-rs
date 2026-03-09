//! Signal-forest preparation before fast-lane FIR lowering.
//!
//! # Source provenance (C++)
//! - `compiler/normalize/normalform.cpp` (`deBruijn2Sym(...)`)
//! - `compiler/box_signal_api.cpp` (`boxesToSignalsMLIR(...)`)
//!
//! # Stage scope
//! This first slice implements the staging-arena boundary and forest-wide
//! `de_bruijn_to_sym` conversion only. Typing and promotion are added in later
//! steps so the fast-lane can move toward:
//!
//! `propagate -> de_bruijn_to_sym -> typing -> promotion -> signal_fir`

use std::error::Error;
use std::fmt;

use signals::SigId;
use tlib::{RecursionError, TreeArena};

/// Prepared signal package consumed by the fast-lane FIR lowerer.
///
/// The package owns a private staging arena so preparation passes can rewrite
/// the signal forest without mutating the original parse/eval arena.
#[derive(Debug)]
pub struct PreparedSignals {
    /// Private staging arena containing the prepared signal forest.
    pub arena: TreeArena,
    /// Prepared output roots interned in [`Self::arena`].
    pub outputs: Vec<SigId>,
}

/// Errors returned while preparing signals for FIR lowering.
#[derive(Debug)]
pub enum SignalPrepareError {
    /// The output forest contains malformed or open de Bruijn recursion.
    Recursion(RecursionError),
}

impl fmt::Display for SignalPrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recursion(err) => write!(
                f,
                "signal preparation failed during de_bruijn_to_sym: {err}"
            ),
        }
    }
}

impl Error for SignalPrepareError {}

impl From<RecursionError> for SignalPrepareError {
    fn from(value: RecursionError) -> Self {
        Self::Recursion(value)
    }
}

/// Clones one output forest into a private arena and converts de Bruijn
/// recursion to symbolic recursion with forest-wide sharing preserved.
///
/// C++ parity note: `deBruijn2Sym(...)` is conceptually applied to the whole
/// output list, not independently to each output root. This function mirrors
/// that contract by cloning all outputs through one memo table and by
/// converting one list root in the staging arena.
pub fn prepare_signals_for_fir(
    src_arena: &TreeArena,
    outputs: &[SigId],
) -> Result<PreparedSignals, SignalPrepareError> {
    let mut arena = TreeArena::new();
    let cloned_outputs = arena.clone_forest_from(src_arena, outputs);
    let cloned_list = vec_to_list(&mut arena, &cloned_outputs);
    let symbolic_list = tlib::de_bruijn_to_sym(&mut arena, cloned_list)?;
    let outputs = list_to_vec(&arena, symbolic_list)
        .expect("prepare_signals_for_fir rebuilds a proper cons list");
    Ok(PreparedSignals { arena, outputs })
}

fn vec_to_list(arena: &mut TreeArena, values: &[SigId]) -> SigId {
    let mut out = arena.nil();
    for value in values.iter().rev() {
        out = arena.cons(*value, out);
    }
    out
}

fn list_to_vec(arena: &TreeArena, mut list: SigId) -> Option<Vec<SigId>> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        out.push(arena.hd(list)?);
        list = arena.tl(list)?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use signals::{SigBuilder, SigMatch, match_sig};
    use tlib::{de_bruijn_rec, de_bruijn_ref, match_sym_rec, match_sym_ref};

    use super::prepare_signals_for_fir;

    #[test]
    fn prepare_signals_for_fir_converts_shared_debruijn_group_once_per_forest() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            let in0 = b.input(0);
            b.add(self_ref, in0)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let (proj0, proj1) = {
            let mut b = SigBuilder::new(&mut arena);
            (b.proj(0, group), b.proj(0, group))
        };

        let prepared =
            prepare_signals_for_fir(&arena, &[proj0, proj1]).expect("closed recursion group");

        assert_eq!(prepared.outputs.len(), 2);
        let SigMatch::Proj(_, left_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
            panic!("expected left projection");
        };
        let SigMatch::Proj(_, right_group) = match_sig(&prepared.arena, prepared.outputs[1]) else {
            panic!("expected right projection");
        };
        assert_eq!(
            left_group, right_group,
            "forest preparation should keep one symbolic group identity across outputs"
        );

        let (var, body_list) =
            match_sym_rec(&prepared.arena, left_group).expect("symbolic recursion expected");
        let body = prepared
            .arena
            .hd(body_list)
            .expect("symbolic body list head");
        let SigMatch::BinOp(_, lhs, rhs) = match_sig(&prepared.arena, body) else {
            panic!("prepared recursive body should stay intact");
        };
        assert_eq!(match_sym_ref(&prepared.arena, lhs), Some(var));
        assert_eq!(match_sig(&prepared.arena, rhs), SigMatch::Input(0));
    }
}
