"""In-place block rendering through the buffer protocol (`compute_into`).

The core cases use only the stdlib (`array.array` reshaped via `memoryview`), so
they run without NumPy. A second group exercises the common NumPy path and is
skipped when NumPy is absent.
"""

import array

import pytest

import faust_rs


def _mv2d(fmt, channels, data):
    """A writable 2-D ``(channels, frames)`` memoryview over fresh storage.

    `data` is a flat, row-major (channel-major) sequence. Reshaping a typed
    `array.array` requires routing through a byte view, since `memoryview.cast`
    only adds/removes dimensions when one side is a byte format.
    """
    buf = array.array(fmt, data)
    return memoryview(buf).cast("B").cast(fmt, [channels, len(data) // channels])


def _flat(mv):
    """Row-major flattening of a 2-D memoryview for value comparison.

    `memoryview.tolist()` materializes a 2-D view as nested lists (chained
    `mv[c][f]` indexing is unsupported for multi-dimensional views).
    """
    return [x for row in mv.tolist() for x in row]


def test_gain_in_place():
    dsp = faust_rs.compile("process = *(0.25);")
    ins = _mv2d("f", 1, [4.0, 8.0, 40.0])
    outs = _mv2d("f", 1, [0.0, 0.0, 0.0])
    dsp.compute_into(ins, outs)
    assert _flat(outs) == [1.0, 2.0, 10.0]


def test_matches_list_compute():
    src = "process = _,_ : + : *(0.5);"
    a = faust_rs.compile(src)
    b = faust_rs.compile(src)
    frames = [1.0, 2.0, 3.0, 4.0]
    expected = a.compute([frames, frames])

    ins = _mv2d("f", 2, frames + frames)  # 2 channels, row-major
    outs = _mv2d("f", 1, [0.0, 0.0, 0.0, 0.0])
    b.compute_into(ins, outs)
    assert _flat(outs) == pytest.approx(expected[0])


def test_state_persists_across_calls():
    # A one-pole integrator: y[n] = x[n] + y[n-1]. Two successive blocks must
    # continue from the accumulated state, matching list-based compute().
    src = "process = + ~ _;"
    a = faust_rs.compile(src)
    b = faust_rs.compile(src)

    block = [1.0, 1.0, 1.0, 1.0]
    exp1 = a.compute([block])
    exp2 = a.compute([block])

    ins = _mv2d("f", 1, block)
    outs = _mv2d("f", 1, [0.0, 0.0, 0.0, 0.0])
    b.compute_into(ins, outs)
    assert _flat(outs) == pytest.approx(exp1[0])
    b.compute_into(ins, outs)
    assert _flat(outs) == pytest.approx(exp2[0])
    assert b.cycle == 2


def test_double_precision_in_place():
    dsp = faust_rs.compile("process = *(0.25);", double=True)
    assert dsp.precision == "double"
    ins = _mv2d("d", 1, [4.0, 8.0, 40.0])
    outs = _mv2d("d", 1, [0.0, 0.0, 0.0])
    dsp.compute_into(ins, outs)
    assert _flat(outs) == [1.0, 2.0, 10.0]


def test_dtype_mismatch_raises():
    # A float (f32) DSP given a float64 buffer must raise, not silently cast.
    dsp = faust_rs.compile("process = *(0.25);")
    ins = _mv2d("d", 1, [4.0, 8.0, 40.0])
    outs = _mv2d("d", 1, [0.0, 0.0, 0.0])
    with pytest.raises(ValueError):
        dsp.compute_into(ins, outs)


def test_wrong_input_channel_count_raises():
    dsp = faust_rs.compile("process = *(0.25);")  # 1 input
    ins = _mv2d("f", 2, [1.0, 2.0])  # 2 channels
    outs = _mv2d("f", 1, [0.0])
    with pytest.raises(ValueError):
        dsp.compute_into(ins, outs)


def test_frame_count_mismatch_raises():
    dsp = faust_rs.compile("process = _;")  # 1 in, 1 out
    ins = _mv2d("f", 1, [1.0, 2.0, 3.0])
    outs = _mv2d("f", 1, [0.0, 0.0])  # fewer frames
    with pytest.raises(ValueError):
        dsp.compute_into(ins, outs)


def test_readonly_output_raises():
    dsp = faust_rs.compile("process = *(0.25);")
    ins = _mv2d("f", 1, [4.0])
    outs = _mv2d("f", 1, [0.0]).toreadonly()
    with pytest.raises(ValueError):
        dsp.compute_into(ins, outs)


def test_one_d_buffer_raises():
    dsp = faust_rs.compile("process = *(0.25);")
    ins = memoryview(array.array("f", [4.0]))  # 1-D, not (channels, frames)
    outs = _mv2d("f", 1, [0.0])
    with pytest.raises(ValueError):
        dsp.compute_into(ins, outs)


# --- NumPy path (individually skipped when NumPy is unavailable) -----------
#
# A module-level `importorskip` would skip the stdlib cases above too, so guard
# only the NumPy tests via a marker.

try:
    import numpy as np
except ImportError:  # pragma: no cover - environment-dependent
    np = None

requires_numpy = pytest.mark.skipif(np is None, reason="numpy is not installed")


@requires_numpy
def test_numpy_gain_in_place():
    dsp = faust_rs.compile("process = *(0.25);")
    ins = np.array([[4.0, 8.0, 40.0]], dtype=np.float32)
    outs = np.zeros((1, 3), dtype=np.float32)
    dsp.compute_into(ins, outs)
    np.testing.assert_array_equal(outs, np.array([[1.0, 2.0, 10.0]], dtype=np.float32))


@requires_numpy
def test_numpy_zero_input_generator():
    # A 0-input generator: pass a (0, frames) input; frames come from outputs.
    dsp = faust_rs.compile("process = 0.7;")
    ins = np.zeros((0, 4), dtype=np.float32)
    outs = np.zeros((1, 4), dtype=np.float32)
    dsp.compute_into(ins, outs)
    np.testing.assert_allclose(outs[0], [0.7, 0.7, 0.7, 0.7], rtol=1e-6)


@requires_numpy
def test_numpy_double_precision():
    dsp = faust_rs.compile("process = *(0.25);", double=True)
    ins = np.array([[4.0, 8.0, 40.0]], dtype=np.float64)
    outs = np.zeros((1, 3), dtype=np.float64)
    dsp.compute_into(ins, outs)
    np.testing.assert_array_equal(outs, np.array([[1.0, 2.0, 10.0]]))


@requires_numpy
def test_numpy_non_contiguous_raises():
    # A column-strided (non-C-contiguous) view must be rejected rather than
    # bulk-copied incorrectly.
    dsp = faust_rs.compile("process = *(0.25);")
    base = np.zeros((1, 6), dtype=np.float32)
    ins = np.array([[4.0, 8.0, 40.0]], dtype=np.float32)
    outs = base[:, ::2]  # non-contiguous
    assert not outs.flags["C_CONTIGUOUS"]
    with pytest.raises(ValueError):
        dsp.compute_into(ins, outs)
