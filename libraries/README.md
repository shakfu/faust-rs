# Project-local Faust libraries

This directory contains Faust libraries that exercise `faust-rs` extensions
and are versioned with this repository:

- `optimizers.lib` provides FAD-based update engines and one- to
  five-parameter optimization loops;
- `interleave.lib` provides frame-rate serialization around `ondemand` blocks.

Add this directory to the Faust import search path when compiling a DSP that
uses either library:

```sh
faust-rs -I libraries -lang cpp program.dsp
cargo run -p compiler -- --check program.dsp -I libraries
```

The library source keeps ordinary basename imports, for example
`import("optimizers.lib")` or `il = library("interleave.lib")`.
