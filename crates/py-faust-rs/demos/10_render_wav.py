"""Render a short melody to WAV, automating parameters between blocks."""

from _shared import OUT_DIR, PRELUDE, write_wav

import faust_rs

SR = 44100
BLOCK = 128

# Two detuned oscillators, a one-pole smoothed gate as envelope, stereo spread.
SRC = PRELUDE + r"""
declare name "voice";
freq = hslider("freq", 220, 20, 2000, 0.01);
smooth(c) = *(1 - c) : + ~ *(c);
env = button("gate") : smooth(0.998);
process = (osc(freq) + osc(freq * 1.005)) * 0.25 * env <: *(0.8), *(0.6);
"""

dsp = faust_rs.compile(SRC, sample_rate=SR, backend="cranelift")

notes = [57, 60, 64, 67, 72, 67, 64, 60]  # MIDI
left, right = [], []
for note in notes:
    dsp.set_param("freq", 440 * 2 ** ((note - 69) / 12))
    for gate, seconds in ((1, 0.2), (0, 0.1)):
        dsp.set_param("gate", gate)
        for _ in range(int(seconds * SR) // BLOCK):
            l, r = dsp.compute([], frames=BLOCK)
            left += l
            right += r

path = write_wav(OUT_DIR / "melody.wav", [left, right], SR)
print(f"wrote {path} ({len(left) / SR:.2f} s, peak {max(map(abs, left)):.3f})")
