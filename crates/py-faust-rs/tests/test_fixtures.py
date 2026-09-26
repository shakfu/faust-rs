"""Real-world `.dsp` file fixtures (vendored from cyfaust).

Only `noise.dsp` is exercised: it is the one cyfaust fixture that does not
`import("stdfaust.lib")`, so it compiles without import-search-path wiring
(a current binding limitation). The stdfaust-dependent fixtures (osc/vco/
soundfile) are intentionally not included here.
"""

import pytest

import faust_rs


def test_noise_dsp_compiles_and_runs(dsp_dir):
    source = (dsp_dir / "noise.dsp").read_text()
    dsp = faust_rs.compile(source, name="Noise")
    assert dsp.name == "Noise"
    # A UI slider is present but does not add audio I/O: 0 inputs, 1 output.
    assert dsp.num_inputs == 0
    assert dsp.num_outputs == 1

    out = dsp.compute([], frames=64)[0]
    assert len(out) == 64
    assert all(-1.0 <= s <= 1.0 for s in out)  # noise stays in range
    assert any(s != 0.0 for s in out)  # and is not pure silence


def test_noise_dsp_is_deterministic(dsp_dir):
    source = (dsp_dir / "noise.dsp").read_text()
    a = faust_rs.compile(source).compute([], frames=32)[0]
    b = faust_rs.compile(source).compute([], frames=32)[0]
    assert a == b  # same seed sequence from a fresh instance


def test_noise_dsp_double_precision(dsp_dir):
    source = (dsp_dir / "noise.dsp").read_text()
    dsp = faust_rs.compile(source, double=True)
    assert dsp.precision == "double"
    out = dsp.compute([], frames=16)[0]
    assert len(out) == 16
