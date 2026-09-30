"""State persists across compute() calls; reset() clears it."""

import faust_rs

# y[n] = y[n-1] + 1
counter = faust_rs.compile("process = +(1) ~ _;")
print("block 1:", counter.compute([], frames=4))
print("block 2:", counter.compute([], frames=4))
print("cycle:", counter.cycle)

counter.reset()
print("after reset:", counter.compute([], frames=4))

# A one-pole lowpass, y[n] = 0.1 x[n] + 0.9 y[n-1]: its step response
# continues smoothly across block boundaries.
lp = faust_rs.compile("process = *(0.1) : + ~ *(0.9);")
step = [1.0] * 4
for i in range(3):
    print(f"lowpass block {i}:", [round(s, 4) for s in lp.compute([step])[0]])
