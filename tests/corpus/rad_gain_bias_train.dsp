// rad on the simplest trainable feed-forward map: out = gain * in + bias.
//
// Seeds (gain, bias) are exposed as UI sliders so a host gradient-descent
// loop can read the gradient bundle, accumulate it over a sample block,
// and update the slider state between blocks.
//
// Output bundle: [out, ∂out/∂gain, ∂out/∂bias]
//   ∂out/∂gain = in
//   ∂out/∂bias = 1
gain = hslider("gain", 1.0, -4.0, 4.0, 0.001);
bias = hslider("bias", 0.0, -4.0, 4.0, 0.001);
process = rad(gain * _ + bias, (gain, bias));
