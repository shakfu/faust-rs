"""Single (f32) vs double (f64) precision."""

import pytest

# 2**24 + 1 is exactly representable in f64 but rounds to 2**24 in f32.
POW24_PLUS_1 = "process = 16777217.0;"


def test_precision_getter(compile_dsp):
    assert compile_dsp("process = _;").precision == "float"
    assert compile_dsp("process = _;", double=True).precision == "double"


def test_float_rounds_pow24_plus_one(compile_dsp):
    out = compile_dsp(POW24_PLUS_1).compute([], frames=1)
    assert out[0][0] == 16777216.0  # rounded down in single precision


def test_double_is_exact_at_pow24_plus_one(compile_dsp):
    out = compile_dsp(POW24_PLUS_1, double=True).compute([], frames=1)
    assert out[0][0] == 16777217.0  # exact in double precision


def test_double_roundtrips_input_losslessly(compile_dsp):
    # A value needing >24 mantissa bits survives f64 marshaling unchanged.
    val = 1.0 + 2.0**-40
    dsp = compile_dsp("process = _;", double=True)
    assert dsp.compute([[val]])[0][0] == val


def test_double_filter_tighter_tolerance(compile_dsp):
    dsp = compile_dsp("process = *(0.1) : +~*(0.9);", double=True)
    r1 = dsp.compute([[1.0, 0.0, 0.0]])[0]
    r2 = dsp.compute([[0.0, 0.0, 0.0]])[0]
    assert r2[0] == pytest.approx(r1[2] * 0.9, rel=1e-12)


def test_both_precisions_persist_independently(compile_dsp):
    a = compile_dsp("process = (+(1))~_;")  # float
    b = compile_dsp("process = (+(1))~_;", double=True)  # double
    a.compute([], frames=10)
    assert b.compute([], frames=2)[0] == [1.0, 2.0]  # b unaffected by a
