//! The execution modes the gate runs programs in, as the command-line flags of the runners.

/// The flag that runs the interpreter alone.
pub const INTERPRETER: &str = "--no-jit";

/// The modes trunk must pass whole suites in: the interpreter, b-jit alone, and the default
/// (every tier), with their runner flag (none for the default).
pub const TRUNK_MODES: [(&str, Option<&str>); 3] = [("no-jit", Some(INTERPRETER)), ("b-jit", Some("--b-jit")), ("default", None)];
