//! The constants the link-layout tools (`symbol-order`, `bolt-profile`) share. The tools read only `nm` and
//! profiler output, so they depend on nothing of the engine: as bins of `quench-wasm-test` they built that
//! package's library and the whole runtime graph under its feature set (7 min) to train a link order.

/// The mangling prefix of Rust symbols (v0).
pub const RUST_SYMBOL_PREFIX: &str = "_R";

/// `nm` symbol kinds that name code.
pub const CODE_KINDS: &[&str] = &["t", "T", "w", "W"];
