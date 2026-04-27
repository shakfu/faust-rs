# graph

Shared graph algorithms crate for compiler passes.

## Intended role

- Host reusable graph data structures and algorithms such as topological order,
  strongly connected components, and scheduling helpers.
- Keep graph-specific logic out of IR crates (`signals`, `fir`, `transform`) so
  crate boundaries stay simple.

## Current status

Scaffold only. No graph API is stabilized yet.

## Public API

| Item | Description |
|---|---|
| `crate_id()` | Returns the stable crate identifier |

