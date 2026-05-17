//! # Rune CLI Library
//!
//! Shared CLI components for the `rune` and `cargo-rune` binaries.

#![cfg_attr(
    not(any(feature = "binary-rune", feature = "binary-cargo")),
    forbid(unsafe_code)
)]

pub mod cli;
