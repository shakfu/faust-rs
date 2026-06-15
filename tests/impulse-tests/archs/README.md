# archs

This directory is intentionally (almost) empty.

The reference impulse responses and the native C/C++ backend tests are wrapped
in the **original C++ Faust 4-pass impulse architecture** (`impulsearch.cpp` +
`controlTools.h`). That architecture `#include`s headers from a C++ Faust source
tree (polyphonic wrapper, MIDI, soundfile, `libfaust.h`, ...), so it cannot be
vendored standalone here.

The makefiles therefore reference it in place via the `IMPULSE_ARCH` /
`CPP_TESTS` / `FAUST_ARCH` variables in [`../common.mk`](../common.mk), which
default to a local Faust checkout. Override them for your environment, e.g.:

```bash
make reference CPP_TESTS=/path/to/faust/tests/impulse-tests \
               FAUST_ARCH=/path/to/faust/architecture \
               FAUST_CPP=/path/to/faust/build/bin/faust
```

faust-rs-native architectures (a self-contained scalar/poly impulse harness that
removes the C++ Faust dependency) are tracked as a future phase in the porting
plan.
