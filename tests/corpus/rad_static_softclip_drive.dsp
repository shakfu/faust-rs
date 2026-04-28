// rad on a stateless soft-clip stage parameterised by drive and bias.
//
//   y = tanh(drive * x + bias) / drive
//
// This is the static (no-feedback) part of many guitar-amp/analog-emu
// chains. Seeds (drive, bias) let a host fit the curve to a recorded
// reference; phase-1 RAD already covers `tanh` (unary FFun) and the
// static composition.
//
// Output bundle: [y, ∂y/∂drive, ∂y/∂bias]
import("stdfaust.lib");
drive = hslider("drive", 1.5, 0.1, 8.0, 0.001);
bias = hslider("bias", 0.0, -2.0, 2.0, 0.001);

clip(x) = ma.tanh(drive * x + bias) / drive;
process = rad(clip(_), (drive, bias));
