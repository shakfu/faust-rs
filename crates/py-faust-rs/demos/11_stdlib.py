"""Use the Faust standard libraries via search_paths (or FAUST_LIB_PATH)."""

import sys

from _shared import OUT_DIR, find_stdlib, write_wav

import faust_rs

libs = find_stdlib()
if libs is None:
    print("skipped: no stdfaust.lib found; set FAUST_LIB_PATH to a Faust libraries directory")
    sys.exit(0)
print("stdlib:", libs)

SRC = r"""
import("stdfaust.lib");
cutoff = hslider("cutoff", 500, 50, 8000, 1) : si.smoo;
process = no.noise * 0.3 : fi.resonlp(cutoff, 8, 1);
"""
SR = 44100
dsp = faust_rs.compile(SRC, sample_rate=SR, search_paths=[str(libs)])
print([p.path for p in dsp.params()])

# Sweep the cutoff from 200 Hz to 4 kHz over two seconds.
out, blocks = [], 2 * SR // 256
for i in range(blocks):
    dsp.set_param("cutoff", 200 * 20 ** (i / blocks))
    out += dsp.compute([], frames=256)[0]
print("wrote", write_wav(OUT_DIR / "sweep.wav", [out], SR))
