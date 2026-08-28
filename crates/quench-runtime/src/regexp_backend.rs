//! Engine boundary for ECMAScript RegExp execution.
//!
//! The VM owns the observable RegExp algorithms; this module owns only the
//! compiled-program representation. Keeping that boundary explicit lets the
//! backend migrate from the temporary compatibility engine to the native
//! parser/IR/interpreter without changing callers such as replace and split.

pub(crate) use regress::{Flags, Match, Regex};
