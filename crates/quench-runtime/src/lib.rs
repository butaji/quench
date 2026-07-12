//! quench-runtime — Rust-native JavaScript runtime targeting 100% test262 ECMAScript conformance.
//!
//! Uses swc for parsing and a custom interpreter for execution.
//!
//! ## Architecture
//!
//! - **Parser**: Uses swc_ecma_parser to parse JS source into swc AST,
//!   then lowers to our smaller runtime AST.
//! - **Value model**: Custom Value enum with object/function/prototype support.
//! - **Interpreter**: Recursive-descent evaluator for the runtime AST.
//! - **Builtins**: Native implementations of console, Object, Array, etc.
//! - **Host API**: Trait-based registration of host functions from the embedding app.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use quench_runtime::Context;
//!
//! let mut ctx = Context::new()?;
//! let result = ctx.eval("1 + 2")?;
//! assert_eq!(result, quench_runtime::Value::Number(3.0));
//! ```

pub mod ast;
pub mod builtins;
pub mod context;
pub mod env;
pub mod eval;
pub mod host;
pub mod interner;
pub mod interpreter;
pub mod lower;
pub mod swc_parse;
pub mod test262;
pub mod value;

// Re-export commonly used types from the context module
pub use ast::Program;
pub use context::Context;
pub use env::Environment;
pub use host::{register_native, HostFunctions};
pub use value::{JsError, Value};
pub use value::{NativeFunction, Object, ObjectKind, ValueFunction};
