import("stdfaust.lib");
// Test Rem with dynamic (non-constant) divisor computed from sample rate
period = max(1, int(min(ma.SR, 192000.0) / 60.0));
counter = (+(1)) ~ %(period);
process = (counter == 0) : float;
