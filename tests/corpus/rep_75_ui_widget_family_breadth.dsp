// Direct coverage for UI widget families not otherwise represented directly in
// the main corpus: button, vslider, nentry, and vbargraph.

gate = button("gate");
pitch = vslider("pitch", 0.0, -12.0, 12.0, 1.0);
delay = nentry("delay", 200.0, 0.0, 1000.0, 1.0);
meter(x) = x : vbargraph("meter", -1.0, 1.0);

process = gate, pitch, delay, meter(_);
