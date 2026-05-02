//! Box-complexity scoring for SVG fold decisions.
//!
//! C++ reference: `compiler/boxes/boxcomplexity.cpp` — `boxComplexity` /
//! `computeBoxComplexity`.

use tlib::TreeArena;

use crate::{BoxId, BoxMatch, match_box};

/// Recursive complexity score of a box expression.
///
/// Used by the SVG draw module to decide whether to fold a sub-diagram into a
/// separate file (`complexity > fold_threshold`) or decorate it in-place.
///
/// Scoring rules (matching C++ `computeBoxComplexity`):
/// - Leaves (all primitives, UI widgets, tables, foreign items) → **1**
/// - Pass-throughs (wire, cut, route, environment) → **0**
/// - Compositions (seq, par, split, merge, rec) → **sum of children**
/// - Groups (vgroup, hgroup, tgroup) → **complexity of body** (transparent)
/// - Symbolic / ondemand / up/downsampling → **1 + child**
/// - Metadata → **complexity of payload** (transparent)
///
/// C++ reference: `boxcomplexity.cpp:77` — `computeBoxComplexity`.
pub fn box_complexity(arena: &TreeArena, b: BoxId) -> usize {
    match match_box(arena, b) {
        // ── Zero-complexity structural nodes ─────────────────────────
        BoxMatch::Cut | BoxMatch::Wire | BoxMatch::Route(..) | BoxMatch::Environment => 0,
        BoxMatch::Slot(_) => 1,

        // ── Leaf nodes: complexity 1 ──────────────────────────────────
        BoxMatch::Int(_) | BoxMatch::Real(_) | BoxMatch::Waveform(_) => 1,

        // binary primitives
        BoxMatch::Add
        | BoxMatch::Sub
        | BoxMatch::Mul
        | BoxMatch::Div
        | BoxMatch::Rem
        | BoxMatch::And
        | BoxMatch::Or
        | BoxMatch::Xor
        | BoxMatch::Lsh
        | BoxMatch::Rsh
        | BoxMatch::Lt
        | BoxMatch::Le
        | BoxMatch::Gt
        | BoxMatch::Ge
        | BoxMatch::Eq
        | BoxMatch::Ne
        | BoxMatch::Pow
        | BoxMatch::Atan2
        | BoxMatch::Fmod
        | BoxMatch::Remainder
        | BoxMatch::Min
        | BoxMatch::Max
        | BoxMatch::Delay => 1,

        // unary math
        BoxMatch::Acos
        | BoxMatch::Asin
        | BoxMatch::Atan
        | BoxMatch::Cos
        | BoxMatch::Sin
        | BoxMatch::Tan
        | BoxMatch::Exp
        | BoxMatch::Log
        | BoxMatch::Log10
        | BoxMatch::Sqrt
        | BoxMatch::Abs
        | BoxMatch::Floor
        | BoxMatch::Ceil
        | BoxMatch::Rint
        | BoxMatch::Round
        | BoxMatch::IntCast
        | BoxMatch::FloatCast
        | BoxMatch::Delay1
        | BoxMatch::Prefix => 1,

        // selects + tables
        BoxMatch::Select2 | BoxMatch::Select3 => 1,
        BoxMatch::ReadOnlyTable | BoxMatch::WriteReadTable => 1,

        // misc primitives
        BoxMatch::AssertBounds
        | BoxMatch::Lowest
        | BoxMatch::Highest
        | BoxMatch::Attach
        | BoxMatch::Enable
        | BoxMatch::Control => 1,

        // UI widgets
        BoxMatch::Button(_)
        | BoxMatch::Checkbox(_)
        | BoxMatch::VSlider(..)
        | BoxMatch::HSlider(..)
        | BoxMatch::NumEntry(..)
        | BoxMatch::VBargraph(..)
        | BoxMatch::HBargraph(..)
        | BoxMatch::Soundfile(..) => 1,

        // foreign
        BoxMatch::FFun(_) | BoxMatch::Ffunction(..) | BoxMatch::FConst(..) | BoxMatch::FVar(..) => {
            1
        }

        // named ident (unresolved reference)
        BoxMatch::Ident(_) => 1,

        // ── Composition: sum of children ─────────────────────────────
        BoxMatch::Seq(a, b) => box_complexity(arena, a) + box_complexity(arena, b),
        BoxMatch::Par(a, b) => box_complexity(arena, a) + box_complexity(arena, b),
        BoxMatch::Split(a, b) => box_complexity(arena, a) + box_complexity(arena, b),
        BoxMatch::Merge(a, b) => box_complexity(arena, a) + box_complexity(arena, b),
        BoxMatch::Rec(a, b) => box_complexity(arena, a) + box_complexity(arena, b),

        // ── Groups: transparent (complexity of body) ──────────────────
        BoxMatch::VGroup(_, body) | BoxMatch::HGroup(_, body) | BoxMatch::TGroup(_, body) => {
            box_complexity(arena, body)
        }

        // ── Wrappers: 1 + child ───────────────────────────────────────
        BoxMatch::Symbolic(_, body) => 1 + box_complexity(arena, body),
        BoxMatch::Ondemand(inner) | BoxMatch::Upsampling(inner) | BoxMatch::Downsampling(inner) => {
            1 + box_complexity(arena, inner)
        }

        // ── Metadata: transparent ─────────────────────────────────────
        BoxMatch::Metadata(a, _) => box_complexity(arena, a),

        // ── Unknown / anything else: treat as 1 ──────────────────────
        _ => 1,
    }
}
