"""Instance independence, determinism, and drop safety.

Mirrors the intent of cyfaust's memory / clone-lifetime tests, exercising the
owning `Dsp` instance under many compiles, moves (into a list), mass drops, and
cross-thread use.
"""

import gc
import threading

COUNTER = "process = (+(1))~_;"


def test_independent_instances_do_not_share_state(compile_dsp):
    a = compile_dsp(COUNTER)
    b = compile_dsp(COUNTER)
    a.compute([], frames=10)
    assert b.compute([], frames=2)[0] == [1.0, 2.0]  # b untouched by a


def test_recompile_is_deterministic(compile_dsp):
    src = "process = *(0.1) : +~*(0.9);"
    first = compile_dsp(src).compute([[1.0, 0.5, 0.25, 0.0]])[0]
    for _ in range(20):
        again = compile_dsp(src).compute([[1.0, 0.5, 0.25, 0.0]])[0]
        assert again == first


def test_mass_create_drop_survivor_still_valid(compile_dsp):
    handles = [compile_dsp(COUNTER) for _ in range(50)]
    keep = handles[25]
    keep.compute([], frames=1)  # advance survivor to cycle 1
    del handles  # drop the other 49
    gc.collect()
    # survivor keeps working and its state after the mass drop is intact
    assert keep.compute([], frames=2)[0] == [2.0, 3.0]


def test_instances_stored_in_container_stay_independent(compile_dsp):
    fleet = [compile_dsp(COUNTER) for _ in range(8)]
    fleet[3].compute([], frames=5)
    assert fleet[3].cycle == 1
    assert fleet[0].cycle == 0  # untouched instance unaffected


def test_usable_from_another_thread(compile_dsp):
    # Compiled on the main thread, computed on a worker: `Dsp` is not
    # `unsendable`, so PyO3 must not refuse the cross-thread access.
    dsp = compile_dsp(COUNTER)
    out = []
    worker = threading.Thread(target=lambda: out.append(dsp.compute([], frames=3)))
    worker.start()
    worker.join()
    assert out == [[[1.0, 2.0, 3.0]]]
