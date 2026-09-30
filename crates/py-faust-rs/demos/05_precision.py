"""Single (default) versus double precision."""

import faust_rs

# 2^24 + 1 is exact in f64 but rounds to 2^24 in f32.
src = "process = 16777217.0;"
f32 = faust_rs.compile(src)
f64 = faust_rs.compile(src, double=True)
print(f32.precision, f32.compute([], frames=1))
print(f64.precision, f64.compute([], frames=1))

# Accumulated error: sum 0.1 ten thousand times.
acc = "process = +(0.1) ~ _;"
for double in (False, True):
    out = faust_rs.compile(acc, double=double).compute([], frames=10000)[0][-1]
    print(f"double={double}: sum = {out!r}  (exact 1000.0)")
