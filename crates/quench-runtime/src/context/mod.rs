//! Runtime context for the JavaScript engine.

use crate::env::Environment;
use crate::interpreter;
use crate::parser;
use crate::value::{JsError, Object, Value};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

thread_local! {
    pub static CURRENT_CONTEXT: RefCell<Option<*mut Context>> = const { RefCell::new(None) };
}

/// Runtime context - holds the execution environment and globals
pub struct Context {
    env: Rc<RefCell<Environment>>,
    pub string_interner: crate::interner::StringInterner,
}

pub mod helpers;
#[cfg(test)]
mod tests;

impl Context {
    /// Create a new runtime context
    pub fn new() -> Result<Self, JsError> {
        interpreter::reset_depth();
        let env = Environment::new();
        let mut ctx = Context {
            env: Rc::new(RefCell::new(env)),
            string_interner: crate::interner::StringInterner::new(),
        };

        // Set thread-local before init_builtins so eval function can access context
        let ctx_ptr: *mut Context = &mut ctx;
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx_ptr);
        });

        helpers::init_builtins(&mut ctx)?;

        // Clear thread-local after init_builtins
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });

        Ok(ctx)
    }

    /// Reset the context to a clean state (useful for testing)
    pub fn reset(&mut self) -> Result<(), JsError> {
        interpreter::reset_depth();
        self.env = Rc::new(RefCell::new(Environment::new()));
        self.string_interner = crate::interner::StringInterner::new();
        // Clear the stale Promise prototype before it is rebuilt by init_builtins
        crate::builtins::promise::clear_promise_proto();
        // Reset global symbol registry for new realm
        crate::builtins::symbol::reset_global_symbol_registry();
        helpers::init_builtins(self)?;
        Ok(())
    }

    /// Evaluate a JavaScript source string using the recursive interpreter.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();

        // Set thread-local for eval function to access this context
        let ctx_ptr: *mut Context = self;
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx_ptr);
        });

        let result = (|| {
            let program = self.parse(source)?;
            // Script code: set `this = globalThis` per ScriptDeclarationInstantiation
            interpreter::eval_program(&program, &mut self.env, Some(source), true)
        })();

        // Microtask checkpoint: drain promise reactions queued during script
        // execution. Reactions can enqueue more microtasks, so drain to a
        // fixpoint (execute_pending_microtasks loops until the queue is empty).
        let microtask_result = crate::builtins::execute_pending_microtasks();

        // Clear thread-local after eval completes
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });

        match result {
            Ok(value) => {
                microtask_result?;
                Ok(value)
            }
            Err(e) => Err(e),
        }
    }

    /// Evaluate an ES module source string using the recursive interpreter.
    pub fn eval_es_module(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();

        // Set thread-local for eval function to access this context
        let ctx_ptr: *mut Context = self;
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx_ptr);
        });

        let result = (|| {
            let program = parser::parse_es_module(source)?;
            // Module code: `this` is undefined (ThisMode::module per ES spec)
            interpreter::set_this_binding(&self.env, Value::Undefined);
            interpreter::eval_program(&program, &mut self.env, Some(source), false)
        })();

        // Microtask checkpoint (see Context::eval)
        let microtask_result = crate::builtins::execute_pending_microtasks();

        // Clear thread-local after eval completes
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });

        match result {
            Ok(value) => {
                microtask_result?;
                Ok(value)
            }
            Err(e) => Err(e),
        }
    }

    /// Parse JavaScript source into an AST using OXC
    pub fn parse(&self, source: &str) -> Result<crate::ast::Program, JsError> {
        parser::parse_script(source)
    }

    /// Parse TypeScript/TSX source into an AST using OXC (strips type annotations)
    pub fn parse_typescript(&self, source: &str) -> Result<crate::ast::Program, JsError> {
        parser::parse_typescript(source)
    }

    /// Evaluate a TypeScript/TSX source string using the recursive interpreter.
    pub fn eval_typescript(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();
        let ctx_ptr: *mut Context = self;
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx_ptr);
        });
        let result = (|| {
            let program = self.parse_typescript(source)?;
            // Script code: set `this = globalThis`
            interpreter::eval_program(&program, &mut self.env, Some(source), true)
        })();
        // Microtask checkpoint (see Context::eval)
        let microtask_result = crate::builtins::execute_pending_microtasks();
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });
        match result {
            Ok(value) => {
                microtask_result?;
                Ok(value)
            }
            Err(e) => Err(e),
        }
    }

    /// Set a global value in the root environment.
    /// Also sets the value on the globalThis object so that globalThis.Array,
    /// globalThis.Object, etc. work correctly (SameValue semantics require
    /// globalThis === this at script level).
    pub fn set_global(&mut self, name: String, value: Value) {
        // Get globalThis before taking mutable borrow of env
        let global_obj = self.get_global("globalThis").and_then(|v| {
            if let Value::Object(obj) = v {
                Some(obj)
            } else {
                None
            }
        });
        let name_for_global = name.clone();
        let value_for_global = value.clone();
        self.env.borrow_mut().define(name, value);
        // Also set on globalThis so globalThis.Array, globalThis.Object etc. work.
        // Use `define` (not `set`) to bypass non-writable descriptors from
        // define_value_prop. Built-in functions must be writable on globalThis
        // per spec; set_global is called for them after globalThis is created.
        if let Some(global_obj) = global_obj {
            let flags = crate::value::PropertyFlags {
                value: Some(value_for_global.clone()),
                writable: true,
                enumerable: false,
                configurable: true,
            };
            global_obj
                .borrow_mut()
                .define(&name_for_global, value_for_global, flags);
        }
    }

    /// Get a global value from the root environment.
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.env.borrow().get(name)
    }

    /// Get the inner environment.
    #[allow(dead_code)]
    pub(crate) fn env(&self) -> &Rc<RefCell<Environment>> {
        &self.env
    }

    /// Register a native function as a global
    pub fn register_native<F>(&mut self, name: &str, f: F)
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        let mut nf = crate::value::NativeFunction::new(f);
        nf.name = name.to_string();
        self.set_global(name.to_string(), Value::NativeFunction(Rc::new(nf)));
    }

    /// Call a global function with arguments
    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, JsError> {
        let func = self
            .get_global(name)
            .ok_or_else(|| JsError(format!("Function not found: {}", name)))?;

        match func {
            Value::Function(f) => helpers::call_js_function(self, &f, args),
            Value::NativeFunction(nf) => nf.call(Value::Undefined, args),
            _ => Err(JsError(format!("{} is not a function", name))),
        }
    }

    /// Check if a global function exists
    pub fn has_function(&self, name: &str) -> bool {
        matches!(
            self.get_global(name),
            Some(Value::Function(_)) | Some(Value::NativeFunction(_))
        )
    }

    /// Load runtime.js from a path using the recursive interpreter.
    pub fn load_runtime_from(&mut self, path: &Path) -> Result<(), JsError> {
        if path.exists() {
            let source = fs::read_to_string(path)
                .map_err(|e| JsError(format!("Failed to read runtime.js: {}", e)))?;
            self.eval(&source)?;
        }
        Ok(())
    }

    /// Register a module's exports for ES module import resolution.
    /// This is useful for testing ES modules without a file system.
    pub fn register_module(&mut self, path: &str, exports: Object) {
        let cache = self.get_global("__quench_modules__");
        if let Some(Value::Object(cache_obj)) = cache {
            cache_obj
                .borrow_mut()
                .set(path, Value::Object(Rc::new(RefCell::new(exports))));
        }
    }

    /// Get a registered module's exports.
    pub fn get_module(&self, path: &str) -> Option<Value> {
        let cache = self.get_global("__quench_modules__")?;
        if let Value::Object(cache_obj) = cache {
            cache_obj.borrow().get(path)
        } else {
            None
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new().expect("Failed to create JS context")
    }
}

/// Get a global value from the current context's globals.
/// Returns None if no context is active.
pub fn get_global_from_context(name: &str) -> Option<Value> {
    let ctx_ptr = CURRENT_CONTEXT.with(|cell| *cell.borrow())?;
    if ctx_ptr.is_null() {
        return None;
    }
    // SAFETY: ctx_ptr is valid because CURRENT_CONTEXT is set during eval.
    let ctx = unsafe { &*ctx_ptr };
    ctx.get_global(name)
}

/// Get the global environment from the current context.
/// Returns None if no context is active.
pub fn get_current_env() -> Option<std::rc::Rc<std::cell::RefCell<Environment>>> {
    let ctx_ptr = CURRENT_CONTEXT.with(|cell| *cell.borrow())?;
    if ctx_ptr.is_null() {
        return None;
    }
    // SAFETY: ctx_ptr is valid because CURRENT_CONTEXT is set during eval.
    let ctx = unsafe { &*ctx_ptr };
    Some(Rc::clone(&ctx.env))
}
