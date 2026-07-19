//! Test suite for the `signal_prepare` staging boundary, grouped by
//! contract: staging normalizations, reduced typing/promotion, recursion
//! closure, and postcondition verification.

mod fixtures;

mod recursion;
mod staging;
mod typing;
mod verify;
