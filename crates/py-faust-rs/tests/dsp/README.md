# DSP fixtures

Self-contained Faust `.dsp` fixtures used by the test suite.

- `noise.dsp` — white-noise generator with an interactive volume slider.
  Vendored from the sibling `cyfaust` project's test fixtures; originally a
  Faust math-documentation example, `(c) GRAME 2009`, BSD-licensed (see the
  `declare` header in the file). It is used here because it is one of the few
  fixtures that does **not** `import("stdfaust.lib")`, so it compiles without
  import-search-path wiring (a current binding limitation).
