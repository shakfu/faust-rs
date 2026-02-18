# fir crate

FIR (Faust Intermediate Representation) construction and matching helpers used by
the compiler/codegen pipeline.

## Type conventions

- `FirType::UI`, `FirType::Sound`, and `FirType::Meta` are API handle kinds
  that are already pointer-shaped in the FIR model.
- Canonical signatures therefore use these variants directly:
  - `buildUserInterface(UI)`
  - `metadata(Meta)`
- Use `FirType::Ptr(...)` only to add explicit pointer indirection beyond that
  base handle level.
- Examples:
  - `UI` maps to `UI*` (C++) / `UIGlue*` (C backend glue layer).
  - `Ptr(UI)` maps to `UI**` / `UIGlue**`.
  - `Ptr(Ptr(FaustFloat))` maps to `FAUSTFLOAT**` (used by `compute` I/O).
