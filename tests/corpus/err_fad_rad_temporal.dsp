// Mixed AD: fad(rad(x', x), x) — the inner rad falls back to
// SigBlockReverseAD (Phase B1), then fad differentiates the carrier.
// Outputs = 4: [Proj(0,BRA), Proj(0,BRA)', Proj(1,BRA), Proj(1,BRA)'].
// (Previously this was expected to error; updated for Phase B1.)
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = fad(rad(x', x), x);
