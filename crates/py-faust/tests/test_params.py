"""UI parameter bridge: listing, get/set by label and path, effect on compute.

The interpreter exposes DSP controls (sliders, buttons, nentries) and output
bargraphs; this maps them to `params()` / `get_param` / `set_param`.
"""

import pytest

import faust_rs

GAIN = 'process = _ * hslider("gain", 1, 0, 2, 0.01);'


def test_params_listing_and_metadata():
    dsp = faust_rs.compile(GAIN)
    ps = dsp.params()
    assert len(ps) == 1
    p = ps[0]
    assert p.label == "gain"
    assert p.path.endswith("/gain")
    assert p.kind == "hslider"
    assert p.is_input is True
    assert p.init == 1.0
    assert p.min == 0.0
    assert p.max == 2.0
    assert p.step == pytest.approx(0.01, abs=1e-6)  # f32-rounded


def test_no_params_for_plain_dsp():
    assert faust_rs.compile("process = _;").params() == []


def test_set_get_by_label():
    dsp = faust_rs.compile(GAIN)
    assert dsp.compute([[2.0, 4.0]]) == [[2.0, 4.0]]  # default gain=1
    dsp.set_param("gain", 0.5)
    assert dsp.get_param("gain") == 0.5
    assert dsp.compute([[2.0, 4.0]]) == [[1.0, 2.0]]


def test_set_by_full_path():
    dsp = faust_rs.compile(GAIN, name="Amp")
    path = dsp.params()[0].path
    assert path == "/Amp/gain"
    dsp.set_param(path, 0.0)
    assert dsp.compute([[2.0, 4.0]]) == [[0.0, 0.0]]


def test_reset_restores_param_defaults():
    dsp = faust_rs.compile(GAIN)
    dsp.set_param("gain", 0.0)
    dsp.reset()
    assert dsp.get_param("gain") == 1.0  # back to init
    assert dsp.compute([[3.0]]) == [[3.0]]


def test_nested_group_path():
    dsp = faust_rs.compile('process = vgroup("amp", _ * hslider("vol", 0.5, 0, 1, 0.01));')
    assert dsp.params()[0].path == "/amp/vol"
    assert dsp.get_param("vol") == 0.5  # leaf label resolves regardless of root


@pytest.mark.parametrize(
    "source,kind",
    [
        ('process = _ * hslider("g", 1, 0, 2, 0.01);', "hslider"),
        ('process = _ * vslider("g", 1, 0, 2, 0.01);', "vslider"),
        ('process = _ * nentry("g", 1, 0, 2, 0.01);', "nentry"),
        ("process = button(\"go\");", "button"),
        ('process = checkbox("on");', "checkbox"),
    ],
)
def test_widget_kinds(source, kind):
    assert faust_rs.compile(source).params()[0].kind == kind


def test_bargraph_is_output_readable_not_settable():
    dsp = faust_rs.compile('process = _ <: attach(_, hbargraph("meter", 0, 1));')
    meter = next(p for p in dsp.params() if p.kind == "hbargraph")
    assert meter.is_input is False
    with pytest.raises(ValueError):
        dsp.set_param("meter", 0.5)  # cannot set an output
    dsp.compute([[0.7, 0.7]])
    assert dsp.get_param("meter") == pytest.approx(0.7)  # reflects last compute


def test_unknown_param_raises_with_listing():
    dsp = faust_rs.compile(GAIN)
    with pytest.raises(ValueError, match="unknown parameter"):
        dsp.set_param("does_not_exist", 1.0)
    with pytest.raises(ValueError):
        dsp.get_param("does_not_exist")


def test_params_work_in_double_precision():
    dsp = faust_rs.compile(GAIN, double=True)
    assert dsp.precision == "double"
    dsp.set_param("gain", 0.25)
    assert dsp.compute([[8.0]]) == [[2.0]]
