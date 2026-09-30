"""Interpreter versus Cranelift JIT: same output, different speed."""

import time

from _shared import PRELUDE

import faust_rs

# Additive synth: 16 partials.
SRC = PRELUDE + "process = par(i, 16, osc(110 * (i + 1)) / (i + 1)) :> *(0.1);"
FRAMES, BLOCKS = 512, 200

results = {}
for backend, opt in (("interp", 0), ("cranelift", 0), ("cranelift", 3)):
    t0 = time.perf_counter()
    dsp = faust_rs.compile(SRC, backend=backend, opt_level=opt)
    t1 = time.perf_counter()
    out = []
    for _ in range(BLOCKS):
        out += dsp.compute([], frames=FRAMES)[0]
    t2 = time.perf_counter()
    results[(backend, opt)] = out
    rt = FRAMES * BLOCKS / dsp.sample_rate / (t2 - t1)
    print(f"{backend:9} opt={opt}: compile {1e3 * (t1 - t0):6.1f} ms, "
          f"render {1e3 * (t2 - t1):6.1f} ms ({rt:5.1f}x real time)")

ref = results[("interp", 0)]
for key, out in results.items():
    err = max(abs(x - y) for x, y in zip(ref, out))
    print(f"max |{key[0]} opt={key[1]} - interp| = {err:.3g}")

# Extra compiler flags pass through verbatim, e.g. vector mode.
vec = faust_rs.compile(SRC, args=["-vec", "-vs", "16"])
err = max(abs(x - y) for x, y in zip(ref[:FRAMES], vec.compute([], frames=FRAMES)[0]))
print(f"max |-vec - scalar| = {err:.3g}")
