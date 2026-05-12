// Legacy fixture name retained for older reports. Current contract:
// differentiating a temporal node (delay1) through `rad` falls back to the
// SigBlockReverseAD carrier rather than surfacing a user error.
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = rad(x', x);
