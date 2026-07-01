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

pub mod ast;
pub mod value;
pub mod env;
pub mod interpreter;
pub mod lower;
pub mod swc_parse;
pub mod builtins;
pub mod host;
pub mod conformance;
pub mod test262;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::cell::RefCell;

pub use value::{Value, JsError};
pub use ast::Program;
pub use host::{HostFunctions, register_native};

// Re-export commonly used types
pub use value::{Object, ObjectKind, ValueFunction, NativeFunction};
pub use env::Environment;

/// Runtime context - holds the execution environment and globals
pub struct Context {
    env: Rc<RefCell<Environment>>,
    globals: HashMap<String, Value>,
}

impl Context {
    /// Create a new runtime context
    pub fn new() -> Result<Self, JsError> {
        interpreter::reset_depth(); // Reset depth for new context
        let env = Environment::new();
        let mut ctx = Context {
            env: Rc::new(RefCell::new(env)),
            globals: HashMap::new(),
        };
        ctx.init_builtins()?;
        Ok(ctx)
    }

    /// Reset the context to a clean state (useful for testing)
    pub fn reset(&mut self) -> Result<(), JsError> {
        interpreter::reset_depth(); // Reset depth counter
        self.env = Rc::new(RefCell::new(Environment::new()));
        self.globals.clear();
        self.init_builtins()?;
        Ok(())
    }

    /// Initialize built-in globals and functions
    fn init_builtins(&mut self) -> Result<(), JsError> {
        // Use the builtins module
        host::register_builtin_functions(self);
        
        // CommonJS module compatibility
        let exports = Object::new(ObjectKind::Ordinary);
        let exports_rc = Rc::new(RefCell::new(exports));
        let module_obj = Object::new(ObjectKind::Ordinary);
        let module_obj = Rc::new(RefCell::new(module_obj));
        module_obj.borrow_mut().set("exports", Value::Object(Rc::clone(&exports_rc)));
        self.set_global("exports".to_string(), Value::Object(Rc::clone(&exports_rc)));
        self.set_global("module".to_string(), Value::Object(module_obj));
        
        // JavaScript globals - globalThis is the global object itself
        let global_obj = Object::new(ObjectKind::Global);
        let global_obj = Rc::new(RefCell::new(global_obj));
        self.set_global("globalThis".to_string(), Value::Object(Rc::clone(&global_obj)));
        self.set_global("undefined".to_string(), Value::Undefined);
        self.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
        self.set_global("NaN".to_string(), Value::Number(f64::NAN));

        Ok(())
    }

    /// Evaluate a JavaScript source string
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth(); // Reset depth for each top-level eval
        let program = self.parse(source)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Parse JavaScript source into an AST using swc
    pub fn parse(&self, source: &str) -> Result<Program, JsError> {
        swc_parse::parse_swc(source)
    }

    /// Set a global value
    pub fn set_global(&mut self, name: String, value: Value) {
        self.globals.insert(name.clone(), value.clone());
        self.env.borrow_mut().define(name, value);
    }

    /// Get a global value
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    /// Register a native function as a global
    pub fn register_native<F>(&mut self, name: &str, f: F)
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        self.set_global(name.to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(f))));
    }

    /// Call a global function with arguments
    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, JsError> {
        let func = self.get_global(name)
            .ok_or_else(|| JsError(format!("Function not found: {}", name)))?;

        match func {
            Value::Function(f) => {
                let closure = Rc::clone(&f.closure);
                let params = f.params.clone();
                
                let mut call_env = Environment::with_parent(Rc::clone(&closure));
                
                for (i, param) in params.iter().enumerate() {
                    let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
                    call_env.declare(param.clone(), arg);
                }
                
                let call_env = Rc::new(RefCell::new(call_env));
                
                if f.is_arrow {
                    if let Some(arrow_body) = &f.arrow_body {
                        match arrow_body.as_ref() {
                            ast::ArrowBody::Expression(expr) => {
                                interpreter::eval_expression(expr, &call_env)
                            }
                            ast::ArrowBody::Block(stmts) => {
                                interpreter::eval_statements(stmts, &call_env, true)
                            }
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                } else {
                    interpreter::eval_statements(&f.body, &call_env, false)
                }
            }
            Value::NativeFunction(nf) => {
                nf.call(args)
            }
            _ => Err(JsError(format!("{} is not a function", name))),
        }
    }

    /// Check if a global function exists
    pub fn has_function(&self, name: &str) -> bool {
        matches!(self.globals.get(name), Some(Value::Function(_)) | Some(Value::NativeFunction(_)))
    }

    /// Load runtime.js from a path
    pub fn load_runtime_from(&mut self, path: &Path) -> Result<(), JsError> {
        if path.exists() {
            let source = fs::read_to_string(path)
                .map_err(|e| JsError(format!("Failed to read runtime.js: {}", e)))?;
            self.eval(&source)?;
        }
        Ok(())
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new().expect("Failed to create JS context")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let ctx = Context::new();
        assert!(ctx.is_ok());
    }

    #[test]
    fn test_globals() {
        let mut ctx = Context::new().unwrap();
        ctx.set_global("test".to_string(), Value::Number(42.0));
        assert_eq!(ctx.get_global("test"), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_eval_simple() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("1 + 2");
        assert!(result.is_ok());
        if let Ok(v) = result {
            assert_eq!(v, Value::Number(3.0));
        }
    }

    #[test]
    fn test_console_exists() {
        let ctx = Context::new().unwrap();
        let console = ctx.get_global("console");
        assert!(console.is_some());
    }

    #[test]
    fn test_global_this_assignment() {
        let mut ctx = Context::new().unwrap();
        // Test that globalThis exists and is an object
        let result = ctx.eval("typeof globalThis");
        assert!(result.is_ok(), "typeof globalThis failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::String("object".to_string()));
        
        // Test that we can assign to globalThis
        let result = ctx.eval("globalThis.testProp = 42");
        assert!(result.is_ok(), "globalThis assignment failed: {:?}", result);
        
        // Test that we can read back the property
        let result = ctx.eval("globalThis.testProp");
        assert!(result.is_ok(), "globalThis read failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_date_prototype_access() {
        let mut ctx = Context::new().unwrap();
        // Test Date.prototype access
        let result = ctx.eval("Date.prototype");
        assert!(result.is_ok(), "Date.prototype failed: {:?}", result);
        
        // Test Date.prototype.toLocaleTimeString
        let result = ctx.eval("Date.prototype.toLocaleTimeString");
        assert!(result.is_ok(), "Date.prototype.toLocaleTimeString failed: {:?}", result);
    }
}
