"""Persistent, stateful instance behavior across compute calls."""

import pytest

import faust_rs

# y[n] = y[n-1] + 1  ->  1, 2, 3, ... (one-sample feedback delay)
COUNTER = "process = (+(1))~_;"
# leaky integrator: y[n] = 0.1*x[n] + 0.9*y[n-1]
LEAKY = "process = *(0.1) : +~*(0.9);"


def test_state_persists_across_calls():
    dsp = faust_rs.compile(COUNTER)
    assert dsp.compute([], frames=4)[0] == [1.0, 2.0, 3.0, 4.0]
    assert dsp.compute([], frames=4)[0] == [5.0, 6.0, 7.0, 8.0]  # continues


def test_cycle_counter_is_monotonic():
    dsp = faust_rs.compile(COUNTER)
    assert dsp.cycle == 0
    dsp.compute([], frames=1)
    dsp.compute([], frames=1)
    assert dsp.cycle == 2


def test_reset_clears_dsp_state_but_not_cycle():
    dsp = faust_rs.compile(COUNTER)
    dsp.compute([], frames=4)  # -> 1..4
    dsp.compute([], frames=4)  # -> 5..8
    dsp.reset()
    # DSP state restarts...
    assert dsp.compute([], frames=4)[0] == [1.0, 2.0, 3.0, 4.0]
    # ...but the bookkeeping cycle counter keeps counting (3 computes total).
    assert dsp.cycle == 3


def test_filter_state_continuous_across_block_boundary():
    dsp = faust_rs.compile(LEAKY)
    r1 = dsp.compute([[1.0, 0.0, 0.0]])[0]  # impulse -> 0.1, 0.09, 0.081
    r2 = dsp.compute([[0.0, 0.0, 0.0]])[0]  # decay continues from r1[-1]
    assert r2[0] == pytest.approx(r1[2] * 0.9, rel=1e-6)
