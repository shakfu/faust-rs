//! Typed errors for the experimental signal->FIR fast-lane.
//!
//! Error codes are stable and machine-friendly so `compiler` can map them to
//! diagnostics consistently while this lane evolves.

use std::fmt::{Display, Formatter};

/// Stable error-code namespace for the signal->FIR fast-lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalFirErrorCode {
    /// Configuration is invalid for the requested compilation.
    InvalidOptions,
    /// Input signal list is empty.
    EmptySignalList,
    /// Requested output arity does not match provided signal count.
    OutputArityMismatch,
}

impl SignalFirErrorCode {
    /// Returns stable textual code for diagnostics and tests.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidOptions => "FRS-SFIR-0001",
            Self::EmptySignalList => "FRS-SFIR-0002",
            Self::OutputArityMismatch => "FRS-SFIR-0003",
        }
    }
}

/// Error returned by `transform::signal_fir` APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalFirError {
    code: SignalFirErrorCode,
    message: String,
}

impl SignalFirError {
    /// Creates a typed signal->FIR fast-lane error.
    #[must_use]
    pub fn new(code: SignalFirErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable error code.
    #[must_use]
    pub fn code(&self) -> SignalFirErrorCode {
        self.code
    }
}

impl Display for SignalFirError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for SignalFirError {}
