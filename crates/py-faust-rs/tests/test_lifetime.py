"""Instance independence, determinism, and drop safety.

Mirrors the intent of cyfaust's memory / clone-lifetime tests, exercising the
safe owning instance (`OwnedFbcDspInstance`) under many compiles, moves (into a
list), and mass drops.
"""

import gc

import faust_rs

COUNTER = "process = (+(1))~_;"


def test_independent_instances_do_not_share_state():
    a = faust_rs.compile(COUNTER)
    b = faust_rs.compile(COUNTER)
    a.compute([], frames=10)
    assert b.compute([], frames=2)[0] == [1.0, 2.0]  # b untouched by a


def test_recompile_is_deterministic():
    src = "process = *(0.1) : +~*(0.9);"
    first = faust_rs.compile(src).compute([[1.0, 0.5, 0.25, 0.0]])[0]
    for _ in range(20):
        again = faust_rs.compile(src).compute([[1.0, 0.5, 0.25, 0.0]])[0]
        assert again == first


def test_mass_create_drop_survivor_still_valid():
    handles = [faust_rs.compile(COUNTER) for _ in range(50)]
    keep = handles[25]
    keep.compute([], frames=1)  # advance survivor to cycle 1
    del handles  # drop the other 49
    gc.collect()
    # survivor keeps working and its state after the mass drop is intact
    assert keep.compute([], frames=2)[0] == [2.0, 3.0]


def test_instances_stored_in_container_stay_independent():
    fleet = [faust_rs.compile(COUNTER) for _ in range(8)]
    fleet[3].compute([], frames=5)
    assert fleet[3].cycle == 1
    assert fleet[0].cycle == 0  # untouched instance unaffected
