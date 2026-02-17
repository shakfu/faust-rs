//! Stable diagnostic code taxonomy.
//!
//! Codes are organized by phase family and intended to be stable across
//! parser/eval/propagate/compiler integrations and test snapshots.

use crate::DiagnosticCode;

pub const SRC_IO_ERROR: DiagnosticCode = DiagnosticCode("FRS-SRC-0001");
pub const SRC_UNRESOLVED_IMPORT: DiagnosticCode = DiagnosticCode("FRS-SRC-0002");
pub const SRC_IMPORT_CYCLE: DiagnosticCode = DiagnosticCode("FRS-SRC-0003");

pub const LEX_INVALID_TOKEN: DiagnosticCode = DiagnosticCode("FRS-LEX-0001");

pub const PARSE_UNEXPECTED_TOKEN: DiagnosticCode = DiagnosticCode("FRS-PARSE-0001");
pub const PARSE_RECOVERY: DiagnosticCode = DiagnosticCode("FRS-PARSE-0002");
pub const PARSE_INVALID_LITERAL: DiagnosticCode = DiagnosticCode("FRS-PARSE-0003");

pub const EVAL_MISSING_PROCESS: DiagnosticCode = DiagnosticCode("FRS-EVAL-0001");
pub const EVAL_UNDEFINED_SYMBOL: DiagnosticCode = DiagnosticCode("FRS-EVAL-0002");
pub const EVAL_ARITY_MISMATCH: DiagnosticCode = DiagnosticCode("FRS-EVAL-0003");
pub const EVAL_ITERATION_INVALID: DiagnosticCode = DiagnosticCode("FRS-EVAL-0004");
pub const EVAL_GENERIC_FAILURE: DiagnosticCode = DiagnosticCode("FRS-EVAL-0099");

pub const PROP_UNSUPPORTED_BOX: DiagnosticCode = DiagnosticCode("FRS-PROP-0001");
pub const PROP_ARITY_MISMATCH: DiagnosticCode = DiagnosticCode("FRS-PROP-0002");
pub const PROP_RECURSION_MISMATCH: DiagnosticCode = DiagnosticCode("FRS-PROP-0003");

pub const COMP_PARSE_FAILED: DiagnosticCode = DiagnosticCode("FRS-COMP-0001");
pub const COMP_EVAL_FAILED: DiagnosticCode = DiagnosticCode("FRS-COMP-0002");
pub const COMP_PROPAGATE_FAILED: DiagnosticCode = DiagnosticCode("FRS-COMP-0003");

/// Returns all built-in stable diagnostic codes.
#[must_use]
pub fn all_codes() -> &'static [DiagnosticCode] {
    &[
        SRC_IO_ERROR,
        SRC_UNRESOLVED_IMPORT,
        SRC_IMPORT_CYCLE,
        LEX_INVALID_TOKEN,
        PARSE_UNEXPECTED_TOKEN,
        PARSE_RECOVERY,
        PARSE_INVALID_LITERAL,
        EVAL_MISSING_PROCESS,
        EVAL_UNDEFINED_SYMBOL,
        EVAL_ARITY_MISMATCH,
        EVAL_ITERATION_INVALID,
        EVAL_GENERIC_FAILURE,
        PROP_UNSUPPORTED_BOX,
        PROP_ARITY_MISMATCH,
        PROP_RECURSION_MISMATCH,
        COMP_PARSE_FAILED,
        COMP_EVAL_FAILED,
        COMP_PROPAGATE_FAILED,
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::all_codes;

    fn is_valid_code(raw: &str) -> bool {
        let mut parts = raw.split('-');
        let Some(prefix) = parts.next() else {
            return false;
        };
        let Some(family) = parts.next() else {
            return false;
        };
        let Some(num) = parts.next() else {
            return false;
        };
        if parts.next().is_some() {
            return false;
        }
        prefix == "FRS"
            && family
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            && num.len() == 4
            && num.bytes().all(|b| b.is_ascii_digit())
    }

    #[test]
    fn all_codes_follow_stable_format() {
        for code in all_codes() {
            assert!(
                is_valid_code(code.0),
                "invalid diagnostic code format: {}",
                code.0
            );
        }
    }

    #[test]
    fn all_codes_are_unique() {
        let mut seen = HashSet::new();
        for code in all_codes() {
            assert!(seen.insert(code.0), "duplicate diagnostic code: {}", code.0);
        }
    }
}
