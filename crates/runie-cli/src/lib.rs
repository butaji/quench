//! # Rune CLI Library
//!
//! Shared CLI components for the `runie` and `cargo-runie` binaries.

#![cfg_attr(
    not(any(feature = "binary-runie", feature = "binary-cargo")),
    forbid(unsafe_code)
)]

pub mod cli;
