//! Core builtins — canonical ops exposed to JavaScript.
//!
//! The `%ops%` object lives here, exposing spec operations to self-hosted builtins.

pub mod ops_wrapper;
#[cfg(test)]
mod tests;
