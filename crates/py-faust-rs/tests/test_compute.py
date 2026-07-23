"""Block rendering with exact output verification.

Unlike cyfaust's interp tests (which stream to a real audio device and assert
only that computation did not crash), these check exact sample values.
"""

import pytest

import faust_rs


def test_gain():
    dsp = faust_rs.compile("process = *(0.25);")
    assert dsp.compute([[4.0, 8.0, 40.0]]) == [[1.0, 2.0, 10.0]]


def test_stereo_mixer():
    # process = _,_ : + : *(0.5)  ->  (a + b) / 2
    dsp = faust_rs.compile("process = _,_ : + : *(0.5);")
    out = dsp.compute([[1.0, 2.0, 3.0, 4.0], [1.0, 2.0, 3.0, 4.0]])
    assert out == [[1.0, 2.0, 3.0, 4.0]]


def test_zero_input_constant_generator():
    dsp = faust_rs.compile("process = 0.7;")
    out = dsp.compute([], frames=4)
    assert out[0] == pytest.approx([0.7, 0.7, 0.7, 0.7])


def test_block_length_inferred_from_inputs():
    dsp = faust_rs.compile("process = _;")
    assert dsp.compute([[1.0, 2.0, 3.0]]) == [[1.0, 2.0, 3.0]]


def test_zero_input_requires_frames():
    dsp = faust_rs.compile("process = 0.7;")
    with pytest.raises(ValueError):
        dsp.compute([])  # no frames given for a 0-input DSP


def test_wrong_channel_count_raises():
    dsp = faust_rs.compile("process = *(0.25);")  # 1 input
    with pytest.raises(ValueError):
        dsp.compute([[1.0], [2.0]])  # 2 channels supplied


def test_ragged_input_channels_raise():
    dsp = faust_rs.compile("process = _,_ : +;")  # 2 inputs
    with pytest.raises(ValueError):
        dsp.compute([[1.0, 2.0], [1.0]])  # unequal channel lengths


def test_zero_frames_is_empty_block():
    dsp = faust_rs.compile("process = 0.7;")
    out = dsp.compute([], frames=0)
    assert out == [[]]
    assert faust_rs.compile("process = 0.7;").num_outputs == 1
