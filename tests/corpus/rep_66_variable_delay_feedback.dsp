// Chained fixed + variable delay in a feedback loop.
// Mirrors the failing case: process = @(100) : @(hslider("Delay",10,0,100,1)).
// Both delay stages share the same fIOTA and are each backed by a
// next_power_of_two-sized circular buffer.
feedback_gain = hslider("feedback", 0.3, 0.0, 0.9, 0.01);
mod_delay = hslider("mod_delay", 10, 0, 100, 1);
process = _ <: _, (@(100) : @(mod_delay) : *(feedback_gain)) : +;
