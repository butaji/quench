//! Custom JavaScript runtime for Quench
//!
//! A minimal recursive-descent interpreter that supports the JS subset
//! used by the Quench compiler and runtime.js.

pub mod ast;
pub mod value;
pub mod env;
pub mod lexer;
pub mod interpreter;
pub mod builtins;
pub mod host;
pub mod lower;
pub mod swc_parse;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::cell::RefCell;

pub use value::{Value, JsError};
pub use ast::Program;

use value::{Value as JsValue, Object, ObjectKind};
use env::Environment as JsEnvironment;

/// Runtime context - holds the execution environment and globals
pub struct Context {
    env: Rc<RefCell<JsEnvironment>>,
    globals: HashMap<String, JsValue>,
}

impl Context {
    /// Create a new runtime context
    pub fn new() -> Result<Self, JsError> {
        let env = JsEnvironment::new();
        let mut ctx = Context {
            env: Rc::new(RefCell::new(env)),
            globals: HashMap::new(),
        };
        ctx.init_builtins()?;
        Ok(ctx)
    }

    /// Initialize built-in globals and functions
    fn init_builtins(&mut self) -> Result<(), JsError> {
        // Use the full builtins module
        builtins::register_builtins(self);
        
        // CommonJS module compatibility (not in builtins)
        let exports = Object::new(ObjectKind::Ordinary);
        let exports_rc = Rc::new(RefCell::new(exports));
        let module_obj = Object::new(ObjectKind::Ordinary);
        let module_obj = Rc::new(RefCell::new(module_obj));
        module_obj.borrow_mut().set("exports", JsValue::Object(Rc::clone(&exports_rc)));
        self.set_global("exports".to_string(), JsValue::Object(Rc::clone(&exports_rc)));
        self.set_global("module".to_string(), JsValue::Object(module_obj));

        Ok(())
    }

    /// Evaluate a JavaScript source string
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        // Parse and evaluate
        let program = self.parse(source)?;

        // Use the shared environment, not a clone
        // This ensures that hoisted functions are shared across all eval calls
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Parse JavaScript source into an AST using swc
    pub fn parse(&self, source: &str) -> Result<Program, JsError> {
        swc_parse::parse_swc(source)
    }

    /// Set a global value
    pub fn set_global(&mut self, name: String, value: JsValue) {
        self.globals.insert(name.clone(), value.clone());
        self.env.borrow_mut().define(name, value);
    }

    /// Get a global value
    pub fn get_global(&self, name: &str) -> Option<JsValue> {
        self.globals.get(name).cloned()
    }

    /// Call a global function with arguments
    pub fn call_function(&mut self, name: &str, args: Vec<JsValue>) -> Result<JsValue, JsError> {
        let func = self.get_global(name)
            .ok_or_else(|| JsError(format!("Function not found: {}", name)))?;

        match func {
            JsValue::Function(f) => {
                let closure = Rc::clone(&f.closure);
                let params = f.params.clone();
                
                // Create new scope for call
                let mut call_env = JsEnvironment::with_parent(Rc::clone(&closure));
                
                // Bind arguments
                for (i, param) in params.iter().enumerate() {
                    let arg = args.get(i).cloned().unwrap_or(JsValue::Undefined);
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
                        Ok(JsValue::Undefined)
                    }
                } else {
                    interpreter::eval_statements(&f.body, &call_env, false)
                }
            }
            JsValue::NativeFunction(nf) => {
                nf.call(args)
            }
            _ => Err(JsError(format!("{} is not a function", name))),
        }
    }

    /// Load runtime.js from the source directory
    pub fn load_runtime(&mut self) -> Result<(), JsError> {
        let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime.js");
        
        if runtime_path.exists() {
            let source = fs::read_to_string(&runtime_path)
                .map_err(|e| JsError(format!("Failed to read runtime.js: {}", e)))?;
            self.eval(&source)?;
        }
        
        Ok(())
    }

    /// Check if a global function exists
    pub fn has_function(&self, name: &str) -> bool {
        matches!(self.globals.get(name), Some(JsValue::Function(_)) | Some(JsValue::NativeFunction(_)))
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
        
        ctx.set_global("test".to_string(), JsValue::Number(42.0));
        assert_eq!(ctx.get_global("test"), Some(JsValue::Number(42.0)));
    }

    #[test]
    fn test_console_exists() {
        let ctx = Context::new().unwrap();
        // console is an object with log method
        let console = ctx.get_global("console");
        assert!(console.is_some(), "console global should exist");
    }

    #[test]
    fn test_console_log() {
        let mut ctx = Context::new().unwrap();
        // Test that console.log works
        let result = ctx.eval("console.log('Hello, World!')");
        assert!(result.is_ok(), "console.log should execute: {:?}", result);
    }

    #[test]
    fn test_eval_simple() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("1 + 2");
        assert!(result.is_ok(), "eval should work: {:?}", result);
        if let Ok(v) = result {
            assert_eq!(v, Value::Number(3.0));
        }
    }

    #[test]
    fn test_eval_console_call() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("console.log('works')");
        assert!(result.is_ok(), "console call should work: {:?}", result);
    }

    #[test]
    fn test_function_declaration() {
        let mut ctx = Context::new().unwrap();
        // Test that function declarations work
        let result = ctx.eval("function foo() { return 42; }; foo()");
        assert!(result.is_ok(), "function declaration should work: {:?}", result);
        if let Ok(v) = result {
            assert_eq!(v, Value::Number(42.0));
        }
    }

    #[test]
    fn test_function_before_assign() {
        let mut ctx = Context::new().unwrap();
        // Test that function is available after declaration
        let result = ctx.eval(r#"
            function renderToString() { return "hello"; }
            renderToString()
        "#);
        assert!(result.is_ok(), "function before assign should work: {:?}", result);
    }
}
