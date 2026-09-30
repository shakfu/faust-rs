"""UI controls: list, read, and set parameters; read bargraph meters."""

from _shared import PRELUDE

import faust_rs

SRC = PRELUDE + r"""
declare name "synth";
freq = hslider("freq[unit:Hz]", 440, 20, 2000, 1);
gain = hslider("gain", 0.5, 0, 1, 0.01);
gate = checkbox("gate");
process = osc(freq) * gain * gate <: attach(_, abs : hbargraph("level", 0, 1));
"""

dsp = faust_rs.compile(SRC, sample_rate=48000)
for p in dsp.params():
    print(p)
    if p.metadata:
        print("   metadata:", p.metadata)

# Keys resolve as the C++ MapUI does: path, then shortname, then label.
dsp.set_param("/synth/gate", 1)
dsp.set_param("gain", 0.8)
dsp.set_param("freq", 1000)
print("freq =", dsp.get_param("freq"))

dsp.compute([], frames=256)
print("level after a block (bargraph):", round(dsp.get_param("level"), 4))

# Bargraphs are outputs: setting one raises.
try:
    dsp.set_param("level", 1.0)
except faust_rs.ReadOnlyParamError as e:
    print("ReadOnlyParamError:", e)

# Values are not clamped to [min, max], matching Faust's setParamValue.
dsp.set_param("gain", 3.0)
print("unclamped gain =", dsp.get_param("gain"))

# reset() restores every control to its init value.
dsp.reset()
print("after reset: gain =", dsp.get_param("gain"), "gate =", dsp.get_param("gate"))
