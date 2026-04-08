//! Recursive-group carrier data model for the `signal_fir` fast-lane.
//!
//! This module owns the explicit recursion abstractions used by `module.rs`
//! during recursive-group lowering:
//!
//! - carrier storage strategy
//! - carrier metadata
//! - canonical resolved carrier references
//! - canonical resolved delayed recursion reads
//!
//! Group scheduling and final FIR orchestration still live in `module.rs`.

use fir::FirType;

/// Storage strategy used by one recursion carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecursionStorageStrategy {
    /// Two-slot carrier:
    /// - current sample in slot `0`
    /// - previous sample in slot `1`
    /// - post-output finalization copy `slot1 = slot0`
    TwoSlotShift,
    /// Circular carrier larger than 2 slots, indexed by the shared global
    /// circular cursor (`fIOTA`).
    Circular,
}

/// Carrier metadata for one output of a recursive group (`SIGPROJ(i, SYMREC(…))`).
///
/// Each output body in a multi-output recursion group gets its own array.
/// The carrier uses one of two storage strategies:
///
/// - [`RecursionStorageStrategy::TwoSlotShift`] for the default 2-slot case
/// - [`RecursionStorageStrategy::Circular`] when accumulated delay analysis
///   upsizes the carrier to serve delayed reads directly
///
/// Source provenance (C++): `signalFIRCompiler.cpp` (`generateRecProj`,
/// `generateRec`), emitted as `fRecN[2]` / `iRecN[2]`.
#[derive(Clone, Debug)]
pub(super) struct RecArrayInfo {
    /// Generated DSP-struct array variable name (e.g. `fRec7`).
    pub(super) name: String,
    /// Element type (`Int32` for integer recursion, `Float32`/`Float64` otherwise).
    pub(super) typ: FirType,
    /// Allocated circular-buffer size in elements (always a power of two).
    ///
    /// Defaults to 2 (current + previous sample). When the recursion output
    /// is consumed by delayed reads, the carrier may be upsized so those reads
    /// can be served directly from the recursion array instead of a separate
    /// delay line.
    pub(super) size: usize,
}

impl RecArrayInfo {
    pub(super) fn storage_strategy(&self) -> RecursionStorageStrategy {
        if self.size == 2 {
            RecursionStorageStrategy::TwoSlotShift
        } else {
            RecursionStorageStrategy::Circular
        }
    }
}

/// Canonically resolved recursion carrier.
#[derive(Clone, Debug)]
pub(super) struct RecursionCarrierRef {
    pub(super) info: RecArrayInfo,
    pub(super) strategy: RecursionStorageStrategy,
}

impl RecursionCarrierRef {
    pub(super) fn new(info: RecArrayInfo) -> Self {
        let strategy = info.storage_strategy();
        Self { info, strategy }
    }
}

/// Canonically resolved delayed recursion read.
#[derive(Clone, Debug)]
pub(super) struct RecursionDelayRef {
    pub(super) carrier: RecursionCarrierRef,
    pub(super) implicit_delay: usize,
}
