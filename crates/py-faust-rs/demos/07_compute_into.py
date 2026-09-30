"""compute_into: render into preallocated buffers (NumPy, array.array, memoryview)."""

import array

import faust_rs

dsp = faust_rs.compile("process = _, _ : + : *(0.5);")  # float32 DSP

try:
    import numpy as np
except ImportError:
    np = None

if np is not None:
    ins = np.array([[1.0, 2.0, 3.0], [3.0, 2.0, 1.0]], dtype=np.float32)  # (channels, frames)
    outs = np.zeros((dsp.num_outputs, ins.shape[1]), dtype=np.float32)
    dsp.compute_into(ins, outs)
    print("numpy:", outs)

    # A float64 buffer on a float32 DSP raises rather than casting.
    try:
        dsp.compute_into(ins.astype(np.float64), outs.astype(np.float64))
    except faust_rs.FaustError as e:
        print(type(e).__name__ + ":", e)

    # Streaming a long signal block by block into one output array.
    lp = faust_rs.compile("process = *(0.01) : + ~ *(0.99);", double=True)
    signal = np.ones((1, 4096))
    rendered = np.empty_like(signal)
    for start in range(0, signal.shape[1], 256):
        blk = slice(start, start + 256)
        out = np.empty((1, 256))
        lp.compute_into(np.ascontiguousarray(signal[:, blk]), out)
        rendered[:, blk] = out
    print("lowpass step response at 0, 255, 256, 4095:", rendered[0, [0, 255, 256, 4095]])
else:
    print("numpy not installed; skipping the numpy part")

# Without NumPy: a flat array.array cast to a 2-D memoryview.
frames = 4
ins = array.array("f", [1, 2, 3, 4] + [4, 3, 2, 1])
outs = array.array("f", [0.0] * frames)
dsp.compute_into(
    memoryview(ins).cast("B").cast("f", (2, frames)),
    memoryview(outs).cast("B").cast("f", (1, frames)),
)
print("array.array:", list(outs))
