//! Soundfile data structure for the interpreter backend.
//!
//! # Source provenance (C++)
//! - `Soundfile` struct in `architecture/faust/sound_player.h`
//! - Per-part metadata (`fLength`, `fSR`, `fOffset`) and channel buffers
//!   (`fBuffers[chan][sample_idx]`).
//!
//! In the Rust interpreter sample data is stored as `f64` regardless of the
//! DSP precision mode; the executor converts on the fly via `FbcReal::from_f64`.

/// Runtime soundfile data: per-part metadata and interleaved channel buffers.
///
/// # C++ parity
/// - `Soundfile` in `architecture/faust/sound_player.h`
/// - `fLength[part]`, `fSR[part]`, `fOffset[part]`, `fBuffers[chan][idx]`
#[derive(Debug, Clone)]
pub struct Soundfile {
    /// Number of audio channels.
    pub num_channels: usize,
    /// Number of parts (independent clips within the soundfile).
    pub num_parts: usize,
    /// Sample count per part: `fLength[part]`.
    pub lengths: Vec<i32>,
    /// Sample rate per part in Hz: `fSR[part]`.
    pub sample_rates: Vec<i32>,
    /// Start offset of each part inside `buffers[chan]`: `fOffset[part]`.
    pub offsets: Vec<i32>,
    /// Per-channel sample data: `buffers[chan][sample_idx]`.
    pub buffers: Vec<Vec<f64>>,
}

impl Soundfile {
    /// Creates a default 1-part, 1-channel silence soundfile.
    ///
    /// This mirrors the C++ `defaultsound` fallback used when no audio file
    /// is provided or when soundfile loading is not yet supported:
    /// zero-length buffer at the standard 44100 Hz sample rate.
    #[must_use]
    pub fn default_silence() -> Self {
        Self {
            num_channels: 1,
            num_parts: 1,
            lengths: vec![0],
            sample_rates: vec![44100],
            offsets: vec![0],
            buffers: vec![vec![]],
        }
    }

    /// Returns the sample at `buffers[chan][offsets[part] + idx]`.
    ///
    /// Out-of-bounds accesses return `0.0` (silence), matching the C++
    /// interpreter's clamped/default behavior.
    #[must_use]
    pub fn read_sample(&self, chan: usize, part: usize, idx: i32) -> f64 {
        let offset = self.offsets.get(part).copied().unwrap_or(0) as usize;
        let sample_idx = offset.saturating_add(idx.max(0) as usize);
        self.buffers
            .get(chan)
            .and_then(|buf| buf.get(sample_idx))
            .copied()
            .unwrap_or(0.0)
    }
}
