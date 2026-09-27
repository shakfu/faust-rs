"""Shared fixtures for the py-faust-rs test suite.

The suite runs against the installed `faust_rs` extension module. Build it into
the active environment first:

    uv run maturin develop --uv   # from crates/py-faust

Pytest skips the whole suite (rather than erroring) if the module is absent, so
a checkout without a build does not produce spurious failures.
"""

import functools
import os
from pathlib import Path

import pytest

faust_rs = pytest.importorskip(
    "faust_rs",
    reason="build the extension first: `uv run maturin develop --uv` in crates/py-faust",
)

DSP_DIR = Path(__file__).parent / "dsp"


def _find_stdfaust_dir() -> Path | None:
    """Locate a directory containing the Faust standard libraries.

    The faust-rs workspace does not bundle the full stdlib, so import-dependent
    tests need one discovered from the environment. Searches, in order:
    `FAUST_LIB_PATH`, the sibling cyfaust project's vendored libraries, and the
    conventional system install locations. Returns `None` if none is found.
    """
    candidates: list[Path] = []
    env = os.environ.get("FAUST_LIB_PATH")
    if env:
        candidates += [Path(p) for p in env.split(os.pathsep) if p]
    candidates += [
        Path.home() / "projects/personal/cyfaust/resources/libraries",
        Path("/usr/local/share/faust"),
        Path("/opt/homebrew/share/faust"),
        Path("/usr/share/faust"),
    ]
    for d in candidates:
        if (d / "stdfaust.lib").is_file():
            return d
    return None


@pytest.fixture(scope="session")
def dsp_dir() -> Path:
    """Directory holding the vendored `.dsp` fixtures."""
    return DSP_DIR


@pytest.fixture(scope="session")
def stdfaust_dir() -> Path:
    """A directory containing `stdfaust.lib`, or skip if unavailable."""
    found = _find_stdfaust_dir()
    if found is None:
        pytest.skip(
            "no Faust standard library found "
            "(set FAUST_LIB_PATH to a directory containing stdfaust.lib)"
        )
    return found


@pytest.fixture
def faust():
    """The imported `faust_rs` module."""
    return faust_rs


@pytest.fixture(params=["interp", "cranelift"])
def backend(request) -> str:
    """Each backend in turn; tests taking `compile_dsp` run once per backend."""
    return request.param


@pytest.fixture
def compile_dsp(backend):
    """`faust_rs.compile` bound to the current `backend`."""
    return functools.partial(faust_rs.compile, backend=backend)


@pytest.fixture
def f64_io(backend, request):
    """Marks a test needing `f64` audio I/O as a strict xfail on the interpreter.

    The `faust` crate drives the interpreter through its C ABI, which exchanges
    `f32` buffers whatever the precision. Strict, so the test fails once
    upstream exchanges `f64` and the marker can go.
    """
    if backend == "interp":
        request.applymarker(
            pytest.mark.xfail(
                strict=True,
                reason="faust crate: interpreter I/O is f32 in double mode",
            )
        )
