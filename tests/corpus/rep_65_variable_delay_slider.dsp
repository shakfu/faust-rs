// Variable delay controlled by a slider.
// Tests that @(hslider(...)) is accepted when the slider provides a bounded
// interval — the delay line is allocated to next_power_of_two(max+1).
delay_samp = hslider("delay[unit:samples]", 1000, 0, 44100, 1);
process = _ : @(delay_samp);
