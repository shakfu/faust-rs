# algebra

Algebra support crate for deterministic Faust compiler rewrites.

## Intended role

- Host reusable algebraic identities and canonicalization helpers shared by
  `eval`, `normalize`, `interval`, and related passes.
- Keep rewrite rule tables in one place when they become shared across crates.
- Avoid circular dependencies between IR crates by keeping this crate focused on
  algebraic utilities rather than owning a full IR.

## Current status

Scaffold only. No public rewrite API is stabilized yet.

## C++ provenance

Algebraic simplification and canonicalization behavior is historically spread
across C++ evaluation, normalization, signal typing, and interval passes.

## Public API

| Item | Description |
|---|---|
| `crate_id()` | Returns the stable crate identifier |

