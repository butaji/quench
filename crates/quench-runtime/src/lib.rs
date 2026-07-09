//! quench-runtime - Custom JavaScript runtime for Quench
//!
//! A minimal interpreter that supports the JS subset used by the Quench
//! compiler and runtime.js. Uses swc for parsing.
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

pub mod arena;
pub mod ast;
pub mod builtins;
pub mod callframe;
pub mod conformance;
pub mod context;
pub mod env;
pub mod eval;
pub mod hir;
pub mod host;
pub mod interner;
pub mod interpreter;
pub mod lower;
pub mod lower_hir;
pub mod nanbox;
pub mod shadow;
pub mod shape;
pub mod stack_machine;
pub mod swc_parse;
pub mod test262;
pub mod value;

// Re-export commonly used types from the context module
pub use context::Context;
pub use value::{Value, JsError};
pub use ast::Program;
pub use host::{HostFunctions, register_native};
pub use value::{Object, ObjectKind, ValueFunction, NativeFunction};
pub use env::Environment;
