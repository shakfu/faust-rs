"""`Factory`: compile once, create independent instances; JSON and metadata."""

import json

import pytest

COUNTER = "process = +~_;"


def test_instances_have_independent_state(faust, backend):
    factory = faust.Factory(COUNTER, backend=backend)
    a = factory.create_dsp_instance()
    b = factory.create_dsp_instance()
    assert a.compute([[1.0] * 3]) == [[1.0, 2.0, 3.0]]
    assert a.compute([[1.0]]) == [[4.0]]
    assert b.compute([[1.0] * 3]) == [[1.0, 2.0, 3.0]]


def test_instance_outlives_factory_handle(faust, backend):
    factory = faust.Factory(COUNTER, backend=backend)
    dsp = factory.create_dsp_instance()
    del factory
    assert dsp.compute([[1.0, 1.0]]) == [[1.0, 2.0]]


def test_sample_rate_per_instance(faust):
    factory = faust.Factory("process = _;")
    assert factory.create_dsp_instance().sample_rate == 48000
    assert factory.create_dsp_instance(44100).sample_rate == 44100


@pytest.mark.parametrize("bad_rate", [0, -1])
def test_bad_sample_rate_raises(faust, bad_rate):
    with pytest.raises(ValueError):
        faust.Factory("process = _;").create_dsp_instance(bad_rate)


def test_factory_properties(faust, backend):
    factory = faust.Factory("process = _;", name="Amp", double=True, backend=backend)
    assert (factory.name, factory.backend, factory.precision) == ("Amp", backend, "double")
    assert repr(factory) == f'Factory(name="Amp", backend="{backend}", precision="double")'


def test_dsp_factory_is_its_program(compile_dsp, backend):
    dsp = compile_dsp(COUNTER, name="Ctr")
    factory = dsp.factory
    assert (factory.name, factory.backend) == ("Ctr", backend)
    sibling = factory.create_dsp_instance()
    dsp.compute([[1.0]])
    assert sibling.compute([[1.0]]) == [[1.0]]  # fresh state


def test_compile_error_from_factory(faust):
    with pytest.raises(faust.CompileError):
        faust.Factory("process = ;")


def test_get_json(faust, backend):
    src = 'process = _ * hslider("gain", 1, 0, 2, 0.01);'
    doc = json.loads(faust.Factory(src, name="Amp", backend=backend).get_json())
    assert (doc["name"], doc["inputs"], doc["outputs"]) == ("Amp", 1, 1)
    (group,) = doc["ui"]
    assert group["items"][0]["address"] == "/Amp/gain"


def test_dsp_metadata_pairs(compile_dsp, backend):
    meta = compile_dsp("process = _;").metadata()
    assert all(isinstance(k, str) and isinstance(v, str) for k, v in meta)
    if backend == "cranelift":
        assert ("backend", "cranelift") in meta


def test_from_file(faust, backend, dsp_dir):
    factory = faust.Factory.from_file(dsp_dir / "noise.dsp", backend=backend)
    assert (factory.name, factory.backend) == ("noise", backend)
    dsp = factory.create_dsp_instance()
    assert (dsp.num_inputs, dsp.num_outputs) == (0, 1)
    assert all(-1.0 <= s <= 1.0 for s in dsp.compute([], frames=64)[0])


def test_from_file_accepts_str(faust, dsp_dir):
    assert faust.Factory.from_file(str(dsp_dir / "noise.dsp")).name == "noise"


def test_from_file_imports_from_its_directory(faust, backend, tmp_path):
    (tmp_path / "helper.lib").write_text("triple = *(3);\n")
    main = tmp_path / "main.dsp"
    main.write_text('import("helper.lib");\nprocess = triple;\n')
    dsp = faust.Factory.from_file(main, backend=backend).create_dsp_instance()
    assert dsp.compute([[1.0, 2.0]]) == [[3.0, 6.0]]
    # The same text compiled from a string does not search that directory.
    with pytest.raises(faust.CompileError):
        faust.Factory(main.read_text(), backend=backend)


def test_from_file_options(faust, dsp_dir):
    factory = faust.Factory.from_file(dsp_dir / "noise.dsp", double=True)
    assert factory.precision == "double"


def test_from_file_missing_raises(faust, tmp_path):
    with pytest.raises(faust.CompileError):
        faust.Factory.from_file(tmp_path / "absent.dsp")


def test_compile_file(faust, backend, dsp_dir):
    dsp = faust.compile_file(dsp_dir / "noise.dsp", sample_rate=44100, backend=backend)
    assert (dsp.name, dsp.backend, dsp.sample_rate) == ("noise", backend, 44100)
    assert len(dsp.compute([], frames=16)[0]) == 16


def test_compile_file_bad_sample_rate_raises(faust, dsp_dir):
    with pytest.raises(ValueError):
        faust.compile_file(dsp_dir / "noise.dsp", sample_rate=0)
