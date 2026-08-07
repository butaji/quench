//! quench-runtime — Rust-native JavaScript runtime targeting 100% test262 ECMAScript conformance.
//!
//! Uses OXC for parsing and a custom tree-walking interpreter for execution.
//!
//! ## Architecture
//!
//! - **Parser**: Uses the OXC parser to parse JS source into the OXC AST,
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
pub mod ir;
pub mod lower;
pub mod parser;
pub mod runtime;
pub mod strict_reserved;
pub mod test262;
pub mod value;

// Re-export commonly used types from the context module
pub use ast::Program;
pub use context::Context;
pub use env::Environment;
pub use host::{register_native, HostFunctions, Test262Host};
pub use runtime::Runtime;
pub use value::{JsError, Value};
pub use value::{NativeFunction, Object, ObjectKind, ValueFunction};

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn generic_runtime_facade_evaluates_javascript() {
        let mut runtime = crate::Runtime::<
            crate::runtime::DefaultHeap,
            crate::runtime::DefaultCollector,
            crate::runtime::DefaultAllocator,
            crate::runtime::DefaultFrames,
            crate::runtime::DefaultExecutor,
            crate::runtime::DefaultExceptions,
            crate::runtime::DefaultEnvironments,
        >::new()
        .unwrap();
        assert_eq!(runtime.eval("40 + 2").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn runtime_accepts_independent_subsystem_components() {
        struct Heap;
        struct Collector;
        struct Allocator;
        struct Frames;
        struct Executor;
        struct Exceptions;
        struct Environments;
        impl crate::runtime::RuntimeComponent for Heap {}
        impl crate::runtime::RuntimeComponent for Collector {}
        impl crate::runtime::RuntimeComponent for Allocator {}
        impl crate::runtime::RuntimeComponent for Frames {}
        impl crate::runtime::RuntimeComponent for Executor {}
        impl crate::runtime::RuntimeComponent for Exceptions {}
        impl crate::runtime::RuntimeComponent for Environments {}
        impl crate::runtime::Heap for Heap {}
        impl crate::runtime::Collector for Collector {}
        impl crate::runtime::Allocator for Allocator {}
        impl crate::runtime::Frames for Frames {}
        impl crate::runtime::Executor for Executor {
            fn execute(
                &mut self,
                context: &mut Context,
                source: &str,
                module: bool,
            ) -> Result<Value, crate::JsError> {
                if module {
                    context.eval_es_module(source)
                } else {
                    context.eval(source)
                }
            }
        }
        impl crate::runtime::Exceptions for Exceptions {}
        impl crate::runtime::Environments for Environments {}

        let mut runtime = crate::Runtime::<
            Heap,
            Collector,
            Allocator,
            Frames,
            Executor,
            Exceptions,
            Environments,
        >::with_components(
            Heap,
            Collector,
            Allocator,
            Frames,
            Executor,
            Exceptions,
            Environments,
        )
        .unwrap();
        let _: &Heap = runtime.heap();
        assert_eq!(runtime.eval("6 * 7").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_context_eval_number() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_context_eval_string() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("\"hello\"");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hello".to_string()));
    }

    #[test]
    fn context_exposes_owned_ir_parsing() {
        let ctx = Context::new().unwrap();
        let ir = ctx.parse_ir("1 + 2").unwrap();
        assert_eq!(ir.statement_count(), 1);
        assert_eq!(
            ctx.parse_module_ir("export default 1;")
                .unwrap()
                .statement_count(),
            1
        );
    }

    #[test]
    fn test_context_eval_boolean() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(ctx.eval("true").unwrap(), Value::Boolean(true));
        assert_eq!(ctx.eval("false").unwrap(), Value::Boolean(false));
    }

    #[test]
    fn test_context_eval_binary_op() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("1 + 2");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Number(3.0));
    }

    #[test]
    fn test_context_eval_identifier() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("undefined");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Undefined);
    }

    #[test]
    fn test_context_eval_null() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("null");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Null);
    }

    #[test]
    fn test_context_eval_assignment() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let x = 5; x");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Number(5.0));
    }

    #[test]
    fn test_context_eval_function_call() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.max(1, 5, 3)");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Number(5.0));
    }

    #[test]
    fn test_value_number_equality() {
        assert_eq!(Value::Number(1.0), Value::Number(1.0));
        assert_ne!(Value::Number(1.0), Value::Number(2.0));
    }

    #[test]
    fn test_value_string_equality() {
        assert_eq!(
            Value::String("a".to_string()),
            Value::String("a".to_string())
        );
        assert_ne!(
            Value::String("a".to_string()),
            Value::String("b".to_string())
        );
    }

    #[test]
    fn test_value_boolean_equality() {
        assert_eq!(Value::Boolean(true), Value::Boolean(true));
        assert_ne!(Value::Boolean(true), Value::Boolean(false));
    }

    #[test]
    fn test_object_kind_derives() {
        use crate::value::ObjectKind;
        assert_eq!(ObjectKind::Ordinary, ObjectKind::Ordinary);
        assert_eq!(ObjectKind::Array, ObjectKind::Array);
        assert_eq!(ObjectKind::Function, ObjectKind::Function);
    }

    #[test]
    fn test_js_error_creation() {
        let err = crate::value::JsError::new("TypeError: test");
        assert!(err.to_string().contains("TypeError"));
    }
}
