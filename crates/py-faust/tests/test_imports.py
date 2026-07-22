"""`import(...)` resolution via `search_paths` and `FAUST_LIB_PATH`.

These tests need the Faust standard library; they skip (via the `stdfaust_dir`
fixture) when none is discoverable. When available they exercise the vendored
stdfaust-dependent fixtures (osc.dsp) that were previously blocked.
"""

import math

import pytest

import faust_rs

OSC = 'import("stdfaust.lib"); process = os.osc(440);'


def test_import_blocked_without_search_paths():
    # No search paths and (typically) no FAUST_LIB_PATH -> import unresolved.
    if "FAUST_LIB_PATH" in __import__("os").environ:
        pytest.skip("FAUST_LIB_PATH set; import would resolve")
    with pytest.raises(ValueError):
        faust_rs.compile(OSC)


def test_osc_resolves_with_search_paths(stdfaust_dir):
    dsp = faust_rs.compile(OSC, sample_rate=48000, search_paths=[str(stdfaust_dir)])
    assert (dsp.num_inputs, dsp.num_outputs) == (0, 1)
    out = dsp.compute([], frames=16)[0]
    assert out[0] == pytest.approx(0.0, abs=1e-4)  # sine starts at 0
    assert all(-1.001 <= s <= 1.001 for s in out)  # bounded
    assert any(abs(s) > 1e-6 for s in out)  # not silent


def test_osc_frequency_is_plausible(stdfaust_dir):
    # One period of a 480 Hz osc at 48 kHz spans exactly 100 samples; the sign
    # of the first step must be positive (rising from 0).
    dsp = faust_rs.compile(
        'import("stdfaust.lib"); process = os.osc(480);',
        sample_rate=48000,
        search_paths=[str(stdfaust_dir)],
    )
    out = dsp.compute([], frames=4)[0]
    assert out[1] > out[0]  # rising
    assert out[1] == pytest.approx(math.sin(2 * math.pi * 480 / 48000), abs=2e-3)


def test_faust_lib_path_env(stdfaust_dir, monkeypatch):
    monkeypatch.setenv("FAUST_LIB_PATH", str(stdfaust_dir))
    dsp = faust_rs.compile(OSC)  # no explicit search_paths -> resolved via env
    assert dsp.num_outputs == 1
    assert len(dsp.compute([], frames=4)[0]) == 4


def test_vendored_osc_dsp_fixture(stdfaust_dir, dsp_dir):
    source = (dsp_dir / "osc.dsp").read_text()
    dsp = faust_rs.compile(source, name="osc", search_paths=[str(stdfaust_dir)])
    assert (dsp.num_inputs, dsp.num_outputs) == (0, 1)
    assert len(dsp.compute([], frames=8)[0]) == 8


def test_double_precision_with_libraries(stdfaust_dir):
    dsp = faust_rs.compile(OSC, double=True, search_paths=[str(stdfaust_dir)])
    assert dsp.precision == "double"
    assert len(dsp.compute([], frames=4)[0]) == 4
