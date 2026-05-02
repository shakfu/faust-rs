//! Error type for the draw crate.
//!
//! C++ reference: `faustexception` in `drawschema.cpp`.

use std::io;

/// Errors that can occur during SVG diagram generation.
#[derive(Debug)]
pub enum DrawError {
    /// I/O failure (file creation, write).
    Io(io::Error),
    /// Schema layout invariant violated.
    Layout(String),
    /// Output format not yet implemented.
    NotSupported(String),
}

impl std::fmt::Display for DrawError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DrawError::Io(e) => write!(f, "draw I/O error: {e}"),
            DrawError::Layout(msg) => write!(f, "draw layout error: {msg}"),
            DrawError::NotSupported(msg) => write!(f, "draw: not supported: {msg}"),
        }
    }
}

impl std::error::Error for DrawError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DrawError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for DrawError {
    fn from(e: io::Error) -> Self {
        DrawError::Io(e)
    }
}
