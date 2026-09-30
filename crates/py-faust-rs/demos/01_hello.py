"""Compile a Faust program from a string and render one block."""

import faust_rs

print("faust_rs", faust_rs.__version__, "on faust-rs", faust_rs.version())

# 2 inputs summed and halved -> 1 output.
dsp = faust_rs.compile("process = _, _ : + : *(0.5);", name="mixer")
print(dsp)
print("inputs/outputs:", dsp.num_inputs, dsp.num_outputs)

# Audio is a list of channels, each a list of samples.
left = [1.0, 0.0, -1.0, 0.5]
right = [1.0, 1.0, 1.0, 0.5]
print("mix:", dsp.compute([left, right]))

# A generator has no inputs, so the frame count must be given.
gen = faust_rs.compile("process = 0.25, -0.25;")
print("constant generator:", gen.compute([], frames=3))
