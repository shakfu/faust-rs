"""One exception class per facade `ErrorKind`, all under `FaustError(ValueError)`."""

import pytest

KINDS = [
    "CompileError",
    "InstantiateError",
    "UnknownParamError",
    "ReadOnlyParamError",
    "BuffersError",
]


@pytest.mark.parametrize("name", KINDS)
def test_hierarchy(faust, name):
    cls = getattr(faust, name)
    assert issubclass(cls, faust.FaustError)
    assert issubclass(faust.FaustError, ValueError)


def test_buffers_error_from_compute(compile_dsp, faust):
    dsp = compile_dsp("process = _;")
    with pytest.raises(faust.BuffersError):
        dsp.compute([[1.0], [2.0]])


def test_buffers_error_from_compute_into(compile_dsp, faust):
    array = pytest.importorskip("array")
    dsp = compile_dsp("process = _;")
    flat = memoryview(array.array("f", [0.0] * 4)).cast("B").cast("f", (2, 2))
    out = memoryview(array.array("f", [0.0] * 2)).cast("B").cast("f", (1, 2))
    with pytest.raises(faust.BuffersError):
        dsp.compute_into(flat, out)  # 2 input channels for a 1-input DSP


def test_argument_errors_are_plain_value_errors(faust):
    # Bad arguments to the binding are not facade errors.
    with pytest.raises(ValueError) as info:
        faust.compile("process = _;", backend="llvm")
    assert not isinstance(info.value, faust.FaustError)
