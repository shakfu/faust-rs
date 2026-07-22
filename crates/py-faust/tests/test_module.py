"""Module-level surface: version, exported names.

Mirrors cyfaust's `test_interp_version` coverage intent, adapted to our module.
"""

import faust_rs


def test_module_exports():
    for name in ("compile", "version", "Dsp"):
        assert hasattr(faust_rs, name), f"missing export: {name}"


def test_dunder_version_nonempty():
    assert isinstance(faust_rs.__version__, str)
    assert faust_rs.__version__


def test_compiler_version_nonempty():
    # cyfaust: `assert get_version()`
    assert isinstance(faust_rs.version(), str)
    assert faust_rs.version()
