// Phase B1 contract: differentiating a temporal node (delay1) through `rad`
// falls back to the SigBlockReverseAD carrier rather than erroring.
// Outputs: [Proj(0, BlockReverseAD), Proj(1, BlockReverseAD)].
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = rad(x', x);
