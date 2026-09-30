"""Helpers shared by the demos: stdlib-free Faust snippets, WAV output, stdlib lookup."""

import os
import struct
import wave
from pathlib import Path

OUT_DIR = Path(__file__).parent / "out"

# Minimal stand-ins for `ma.SR` and `os.osc`, so most demos need no Faust stdlib.
PRELUDE = r"""
SR = min(192000.0, max(1.0, fconstant(int fSamplingFreq, <math.h>)));
PI = 3.141592653589793;
frac(x) = x - floor(x);
phasor(f) = f / SR : (+ : frac) ~ _;
osc(f) = sin(2 * PI * phasor(f));
"""


def write_wav(path, channels, sample_rate):
    """Write float channels (list of equal-length sequences in [-1, 1]) as 16-bit PCM."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    frames = len(channels[0])
    data = bytearray()
    for i in range(frames):
        for ch in channels:
            s = max(-1.0, min(1.0, float(ch[i])))
            data += struct.pack("<h", int(s * 32767))
    with wave.open(str(path), "wb") as w:
        w.setnchannels(len(channels))
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(bytes(data))
    return path


def find_stdlib():
    """Return a directory containing `stdfaust.lib`, or None."""
    candidates = [Path(p) for p in os.environ.get("FAUST_LIB_PATH", "").split(os.pathsep) if p]
    candidates += [
        Path("/usr/local/share/faust"),
        Path("/opt/homebrew/share/faust"),
        Path("/usr/share/faust"),
    ]
    return next((d for d in candidates if (d / "stdfaust.lib").is_file()), None)
