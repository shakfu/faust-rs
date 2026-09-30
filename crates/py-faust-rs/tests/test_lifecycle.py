"""The C++ `dsp` lifecycle calls: init, instance_init and its three parts."""

import pytest

# y[n] = y[n-1] + g: state from the recursion, a parameter from the slider.
ACC = 'process = hslider("g", 1, 0, 10, 1) : +~_;'


def test_instance_clear_keeps_params(compile_dsp):
    dsp = compile_dsp(ACC)
    dsp.set_param("g", 2)
    dsp.compute([], frames=3)
    dsp.instance_clear()
    assert dsp.get_param("g") == 2
    assert dsp.compute([], frames=3)[0] == [2.0, 4.0, 6.0]


def test_instance_reset_user_interface_keeps_state(compile_dsp):
    dsp = compile_dsp(ACC)
    dsp.set_param("g", 2)
    dsp.compute([], frames=2)  # -> 2, 4
    dsp.instance_reset_user_interface()
    assert dsp.get_param("g") == 1
    assert dsp.compute([], frames=2)[0] == [5.0, 6.0]


@pytest.mark.parametrize("method", ["init", "instance_init"])
def test_init_changes_rate_and_resets(method, compile_dsp):
    dsp = compile_dsp(ACC, sample_rate=48000)
    dsp.set_param("g", 2)
    dsp.compute([], frames=2)
    getattr(dsp, method)(44100)
    assert dsp.sample_rate == 44100
    assert dsp.get_param("g") == 1
    assert dsp.compute([], frames=2)[0] == [1.0, 2.0]


def test_instance_constants_changes_rate_keeps_rest(compile_dsp):
    dsp = compile_dsp(ACC, sample_rate=48000)
    dsp.set_param("g", 2)
    dsp.compute([], frames=2)  # -> 2, 4
    dsp.instance_constants(44100)
    assert dsp.sample_rate == 44100
    assert dsp.get_param("g") == 2
    assert dsp.compute([], frames=1)[0] == [6.0]


def test_instance_constants_updates_rate_constants(compile_dsp):
    dsp = compile_dsp('process = fconstant(int fSamplingFreq, <math.h>);', sample_rate=48000)
    assert dsp.compute([], frames=1) == [[48000.0]]
    dsp.instance_constants(44100)
    assert dsp.compute([], frames=1) == [[44100.0]]


@pytest.mark.parametrize("method", ["init", "instance_init", "instance_constants"])
@pytest.mark.parametrize("rate", [0, -1])
def test_non_positive_rate_refused(method, rate, compile_dsp):
    dsp = compile_dsp(ACC)
    with pytest.raises(ValueError, match="sample_rate"):
        getattr(dsp, method)(rate)
    assert dsp.sample_rate == 48000
