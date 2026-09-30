"""The C++ dsp lifecycle calls: which reset rate, parameters, and state."""

import faust_rs

# A counter scaled by a slider, so state and parameter changes are visible.
dsp = faust_rs.compile('process = (+(1) ~ _) * hslider("k", 1, 0, 10, 1);', sample_rate=48000)


def show(label):
    print(f"{label:34} sr={dsp.sample_rate} k={dsp.get_param('k'):g} "
          f"out={dsp.compute([], frames=3)[0]}")


dsp.set_param("k", 2)
show("start (k=2)")
dsp.instance_clear()
show("instance_clear: state only")
dsp.instance_reset_user_interface()
show("instance_reset_user_interface")
dsp.set_param("k", 3)
dsp.instance_constants(44100)
show("instance_constants(44100)")
dsp.set_param("k", 3)
dsp.instance_init(22050)
show("instance_init(22050): everything")
dsp.set_param("k", 3)
dsp.init(96000)
show("init(96000)")
dsp.reset()
show("reset(): init at current rate")
