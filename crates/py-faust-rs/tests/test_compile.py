"""Compilation from source strings, metadata, and error handling.

The self-contained DSP snippets are drawn from cyfaust's interp tests (e.g.
`process = 0.5,0.6;`, `process = _,3.14 : +;`), but here we additionally assert
the resulting audio-channel layout rather than only that a factory is non-null.
"""

import faust_rs
import pytest

# (source, expected_num_inputs, expected_num_outputs)
SNIPPETS = [
    ("process = 0.5,0.6;", 0, 2),  # cyfaust test_interp_..._from_string1
    ("process = _,3.14 : +;", 1, 1),  # cyfaust generate_auxfiles example
    ("process = _,_ : +;", 2, 1),
    ("process = *(0.25);", 1, 1),
    ("process = 0.7;", 0, 1),
    ("process = _;", 1, 1),
]


@pytest.mark.parametrize("source,num_in,num_out", SNIPPETS)
def test_compile_channel_layout(source, num_in, num_out, compile_dsp):
    dsp = compile_dsp(source)
    assert (dsp.num_inputs, dsp.num_outputs) == (num_in, num_out)


def test_compile_metadata(compile_dsp):
    dsp = compile_dsp("process = _;", name="MyDsp", sample_rate=44100)
    assert dsp.name == "MyDsp"
    assert dsp.sample_rate == 44100
    assert dsp.precision == "float"
    assert dsp.cycle == 0
    assert "MyDsp" in repr(dsp)


def test_default_name_and_sample_rate(compile_dsp):
    dsp = compile_dsp("process = _;")
    assert dsp.name == "FaustDSP"
    assert dsp.sample_rate == 48000


@pytest.mark.parametrize(
    "bad_source",
    [
        "process = ;",  # syntax error
        "process = undefined_symbol;",  # unbound reference
        "",  # empty
    ],
)
def test_compile_bad_source_raises(bad_source, compile_dsp):
    with pytest.raises(ValueError):
        compile_dsp(bad_source)


@pytest.mark.parametrize("bad_rate", [0, -1, -48000])
def test_compile_bad_sample_rate_raises(bad_rate, compile_dsp):
    with pytest.raises(ValueError):
        compile_dsp("process = _;", sample_rate=bad_rate)


def test_default_backend_is_interp():
    assert faust_rs.compile("process = _;").backend == "interp"


def test_backend_getter(compile_dsp, backend):
    dsp = compile_dsp("process = _;")
    assert dsp.backend == backend
    assert backend in repr(dsp)


def test_unknown_backend_raises():
    with pytest.raises(ValueError, match="unknown backend"):
        faust_rs.compile("process = _;", backend="llvm")
