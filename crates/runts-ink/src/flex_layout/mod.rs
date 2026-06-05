//! Layout engine dispatcher for `runts-ink`.
//!
//! The crate supports two layout backends:
//!
//! * **Taffy** (default) — pure-Rust flexbox/grid engine.
//!   Enable with the `taffy` feature (on by default).
//! * **Yoga** — Facebook's C++ flexbox engine, also used by Ink.
//!   Enable with the `yoga` feature.
//!
//! The two features are mutually exclusive.  Only one may be
//! enabled at a time.

#[cfg(all(feature = "taffy", feature = "yoga"))]
compile_error!("Only one of features `taffy` and `yoga` may be enabled at a time.");

#[cfg(not(any(feature = "taffy", feature = "yoga")))]
compile_error!("One of features `taffy` or `yoga` must be enabled for runts-ink.");

#[cfg(feature = "taffy")]
mod taffy;
#[cfg(feature = "yoga")]
mod yoga;

#[cfg(feature = "taffy")]
pub use taffy::*;
#[cfg(feature = "yoga")]
pub use yoga::*;
