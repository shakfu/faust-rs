// Minimal soundfile primitive regression.
//
// `soundfile(label, chan)` lowers to:
// - length
// - sample rate
// - one buffer per channel
//
// With `chan = 2`, the process has 4 outputs.

process = soundfile("sample[url:{'demo.wav'}]", 2);
