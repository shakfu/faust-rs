"""Shared fixtures for the faust-rs-py test suite.

The suite runs against the installed `faust_rs` extension module. Build it into
the active environment first:

    maturin develop           # from crates/py-faust

Pytest skips the whole suite (rather than erroring) if the module is absent, so
a checkout without a build does not produce spurious failures.
"""

from pathlib import Path

import pytest

faust_rs = pytest.importorskip(
    "faust_rs",
    reason="build the extension first: `maturin develop` in crates/py-faust",
)

DSP_DIR = Path(__file__).parent / "dsp"


@pytest.fixture(scope="session")
def dsp_dir() -> Path:
    """Directory holding the vendored `.dsp` fixtures."""
    return DSP_DIR


@pytest.fixture
def faust():
    """The imported `faust_rs` module."""
    return faust_rs
