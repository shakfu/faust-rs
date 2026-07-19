//! Vocabulary-only helpers shared across vector pipeline stages.
//!
//! Only items that cannot collapse a trust boundary belong here: total
//! conversions, pure indexing, and domain axioms. Producer/checker
//! reconstructions (reachability, effect summaries, expected results) must
//! stay local to their stage — see `vector/mod.rs`.

pub(crate) mod ids;
