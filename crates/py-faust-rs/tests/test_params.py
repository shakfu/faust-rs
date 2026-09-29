"""UI parameter bridge: listing, get/set by label and path, effect on compute.

Each backend exposes DSP controls (sliders, buttons, nentries) and output
bargraphs; this maps them to `params()` / `get_param` / `set_param`.
"""

import pytest

GAIN = 'process = _ * hslider("gain", 1, 0, 2, 0.01);'


def test_params_listing_and_metadata(compile_dsp):
    dsp = compile_dsp(GAIN)
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


def test_no_params_for_plain_dsp(compile_dsp):
    assert compile_dsp("process = _;").params() == []


def test_set_get_by_label(compile_dsp):
    dsp = compile_dsp(GAIN)
    assert dsp.compute([[2.0, 4.0]]) == [[2.0, 4.0]]  # default gain=1
    dsp.set_param("gain", 0.5)
    assert dsp.get_param("gain") == 0.5
    assert dsp.compute([[2.0, 4.0]]) == [[1.0, 2.0]]


def test_set_by_full_path(compile_dsp):
    dsp = compile_dsp(GAIN, name="Amp")
    path = dsp.params()[0].path
    assert path == "/Amp/gain"
    dsp.set_param(path, 0.0)
    assert dsp.compute([[2.0, 4.0]]) == [[0.0, 0.0]]


def test_reset_restores_param_defaults(compile_dsp):
    dsp = compile_dsp(GAIN)
    dsp.set_param("gain", 0.0)
    dsp.reset()
    assert dsp.get_param("gain") == 1.0  # back to init
    assert dsp.compute([[3.0]]) == [[3.0]]


def test_nested_group_path(compile_dsp):
    dsp = compile_dsp('process = vgroup("amp", _ * hslider("vol", 0.5, 0, 1, 0.01));')
    assert dsp.params()[0].path == "/amp/vol"
    assert dsp.get_param("vol") == 0.5  # leaf label resolves regardless of root


@pytest.mark.parametrize(
    "source,kind",
    [
        ('process = _ * hslider("g", 1, 0, 2, 0.01);', "hslider"),
        ('process = _ * vslider("g", 1, 0, 2, 0.01);', "vslider"),
        ('process = _ * nentry("g", 1, 0, 2, 0.01);', "nentry"),
        ('process = button("go");', "button"),
        ('process = checkbox("on");', "checkbox"),
    ],
)
def test_widget_kinds(source, kind, compile_dsp):
    assert compile_dsp(source).params()[0].kind == kind


def test_bargraph_is_output_readable_not_settable(compile_dsp, faust):
    dsp = compile_dsp('process = _ <: attach(_, hbargraph("meter", 0, 1));')
    meter = next(p for p in dsp.params() if p.kind == "hbargraph")
    assert meter.is_input is False
    with pytest.raises(faust.ReadOnlyParamError):
        dsp.set_param("meter", 0.5)  # cannot set an output
    dsp.compute([[0.7, 0.7]])
    assert dsp.get_param("meter") == pytest.approx(0.7)  # reflects last compute


def test_unknown_param_raises_with_listing(compile_dsp, faust):
    dsp = compile_dsp(GAIN)
    with pytest.raises(faust.UnknownParamError, match="unknown parameter.*/gain"):
        dsp.set_param("does_not_exist", 1.0)
    with pytest.raises(faust.UnknownParamError):
        dsp.get_param("does_not_exist")


def test_params_work_in_double_precision(compile_dsp):
    dsp = compile_dsp(GAIN, double=True)
    assert dsp.precision == "double"
    dsp.set_param("gain", 0.25)
    assert dsp.compute([[8.0]]) == [[2.0]]


def test_label_as_declared(compile_dsp):
    # The path mangles ' ' and drops metadata; the label keeps the text.
    dsp = compile_dsp('process = _ * hslider("my gain[unit:dB]", 1, 0, 2, 0.01);')
    (p,) = dsp.params()
    assert p.label == "my gain"
    assert p.path.endswith("/my_gain")
    dsp.set_param("my gain", 0.5)
    assert dsp.get_param(p.path) == 0.5


def test_params_in_ui_order(compile_dsp):
    # Faust sorts a group by raw label, `[n]` included; paths drop `[n]`.
    # UI order is then b, a; path order would be a, b.
    src = 'process = hslider("[2]a", 0, 0, 1, 0.1) + hslider("[1]b", 0, 0, 1, 0.1);'
    assert [p.label for p in compile_dsp(src).params()] == ["b", "a"]


def test_double_ranges_exact(compile_dsp):
    dsp = compile_dsp('process = _ * hslider("g", 0.1, 0, 2, 0.01);', double=True)
    (p,) = dsp.params()
    assert (p.init, p.step) == (0.1, 0.01)


# Two controls labelled `freq`, told apart by their groups.
TWO_FREQ = (
    'process = hgroup("a", hslider("freq[unit:Hz]", 1, 0, 2, 0.1))'
    ' + hgroup("b", hslider("freq", 1, 0, 2, 0.1));'
)


def test_shortname(compile_dsp):
    assert [p.shortname for p in compile_dsp(GAIN).params()] == ["gain"]
    assert [p.shortname for p in compile_dsp(TWO_FREQ).params()] == ["a_freq", "b_freq"]


def test_lookup_by_shortname(compile_dsp):
    dsp = compile_dsp(TWO_FREQ, name="S")
    dsp.set_param("a_freq", 0.5)
    dsp.set_param("b_freq", 0.25)
    assert dsp.get_param("/S/a/freq") == 0.5
    assert dsp.get_param("/S/b/freq") == 0.25


def test_shared_label_designates_last_declared(compile_dsp):
    # As the C++ MapUI: a label several parameters share is the last one's.
    dsp = compile_dsp(TWO_FREQ, name="S")
    dsp.set_param("freq", 0.5)
    assert dsp.get_param("b_freq") == 0.5
    assert dsp.get_param("a_freq") == 1.0


def test_param_metadata(compile_dsp):
    a, b = compile_dsp(TWO_FREQ).params()
    assert a.metadata == [("unit", "Hz")]
    assert b.metadata == []
