"""Compile once with Factory, then create independent instances."""

import json
from pathlib import Path

from _shared import PRELUDE

import faust_rs

factory = faust_rs.Factory(PRELUDE + "process = phasor(1000);", name="saw")
print(factory, factory.name, factory.backend, factory.precision)

# Each instance has its own state and sample rate.
a = factory.create_dsp_instance(48000)
b = factory.create_dsp_instance(8000)
print("48 kHz:", [round(s, 4) for s in a.compute([], frames=4)[0]])
print(" 8 kHz:", [round(s, 4) for s in b.compute([], frames=4)[0]])
print("instance -> factory:", a.factory.name)

# The factory JSON carries the UI tree and metadata (getDSPFactoryJSON).
ui = json.loads(faust_rs.Factory('process = _ * hslider("vol", 1, 0, 1, 0.1);').get_json())
print("JSON ui:", json.dumps(ui["ui"], indent=1))

# From a file: the factory is named after the file, and the file's directory
# joins the import search path.
dsp_file = Path(__file__).parent / "dsp" / "noise.dsp"
noise = faust_rs.Factory.from_file(str(dsp_file))
print("from_file:", noise.name)
n = faust_rs.compile_file(str(dsp_file), sample_rate=44100)
print("noise block:", [round(s, 4) for s in n.compute([], frames=4)[0]])
