//! Runtime context for the JavaScript engine.

use crate::env::Environment;
use crate::interpreter;
use crate::parser;
use crate::value::{JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

thread_local! {
    pub static CURRENT_CONTEXT: RefCell<Option<*mut Context>> = const { RefCell::new(None) };
    /// Source text of the script/module currently being evaluated.
    /// Set at Context::eval() entry, cleared on exit.
    pub static CURRENT_SOURCE: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}

/// Get the source text of the currently executing script/module.
pub fn current_source() -> Option<&'static str> {
    CURRENT_SOURCE.with(|cell| *cell.borrow())
}

/// A restorable snapshot of the runtime's current realm state.
pub struct RealmSnapshot(intrinsics::IntrinsicSnapshot);

impl RealmSnapshot {
    /// Capture the current realm state.
    pub fn capture() -> Self {
        Self(intrinsics::IntrinsicSnapshot::save())
    }

    /// Restore the captured realm state.
    pub fn restore(self) {
        self.0.restore();
    }
}

/// Runtime context - holds the execution environment and globals
pub struct Context {
    env: Rc<RefCell<Environment>>,
    pub string_interner: crate::interner::StringInterner,
    builtins_bootstrapped: bool,
}

pub mod helpers;
pub(crate) mod intrinsics;
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
            builtins_bootstrapped: false,
        };

        // Set thread-local before init_builtins so eval function can access context
        let ctx_ptr: *mut Context = &mut ctx;
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx_ptr);
        });

        intrinsics::clear_intrinsics();
        helpers::init_builtins(&mut ctx)?;
        ctx.builtins_bootstrapped = true;

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
        self.builtins_bootstrapped = false;
        // Clear every thread-local intrinsic cache before init_builtins rebuilds
        // them, so a reset context never hands out previous-realm intrinsics.
        intrinsics::clear_intrinsics();
        // Reset global symbol registry for new realm
        crate::builtins::symbol::reset_global_symbol_registry();
        helpers::init_builtins(self)?;
        self.builtins_bootstrapped = true;
        Ok(())
    }

    pub(crate) fn builtins_are_bootstrapped(&self) -> bool {
        self.builtins_bootstrapped
    }

    /// Evaluate a JavaScript source string using the recursive interpreter.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();
        let _ = crate::value::error::take_thrown_value();
        let _ = crate::interpreter::take_generator_yield();

        // Set thread-local for eval function to access this context
        let ctx_ptr: *mut Context = self;
        let previous_context = CURRENT_CONTEXT.with(|cell| {
            let previous = *cell.borrow();
            *cell.borrow_mut() = Some(ctx_ptr);
            previous
        });

        // Set source for function source_text capture
        CURRENT_SOURCE.with(|cell| {
            *cell.borrow_mut() = Some(unsafe { std::mem::transmute::<&str, &str>(source as &str) });
        });

        let result = (|| {
            let program = self.parse(source)?;
            helpers::reject_global_lexical_declarations(self, &program)?;
            // Script code: set `this = globalThis` per ScriptDeclarationInstantiation
            interpreter::eval_program(&program, &mut self.env, Some(source), true)
        })();

        // Microtask checkpoint: drain promise reactions queued during script
        // execution. Reactions can enqueue more microtasks, so drain to a
        // fixpoint (execute_pending_microtasks loops until the queue is empty).
        let microtask_result = crate::builtins::execute_pending_microtasks();

        // Clear thread-local after eval completes
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = previous_context;
        });
        CURRENT_SOURCE.with(|cell| {
            *cell.borrow_mut() = None;
        });
        let _ = crate::interpreter::take_generator_yield();

        match result {
            Ok(value) => {
                microtask_result?;
                Ok(value)
            }
            Err(e) => Err(e),
        }
    }

    /// Evaluate a script with an explicit strictness mode.
    pub fn eval_script(&mut self, source: &str, strict: bool) -> Result<Value, JsError> {
        let previous_strict = interpreter::is_strict_mode();
        let previous_direct_eval = interpreter::is_direct_eval();
        interpreter::set_strict_mode(strict);
        interpreter::set_direct_eval(false);
        let result = self.eval(source);
        interpreter::set_strict_mode(previous_strict);
        interpreter::set_direct_eval(previous_direct_eval);
        result
    }

    /// Evaluate an ES module source string using the recursive interpreter.
    pub fn eval_es_module(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();

        // Set thread-locals for eval function to access this context and source
        let ctx_ptr: *mut Context = self;
        let previous_context = CURRENT_CONTEXT.with(|cell| {
            let previous = *cell.borrow();
            *cell.borrow_mut() = Some(ctx_ptr);
            previous
        });
        CURRENT_SOURCE.with(|cell| {
            *cell.borrow_mut() = Some(unsafe { std::mem::transmute::<&str, &str>(source) });
        });

        let result = (|| {
            let program = parser::parse_es_module(source)?;
            if let (Some(Value::String(module)), Some(Value::Object(errors))) = (
                self.env.borrow().get("__quench_current_module__"),
                self.env.borrow().get("__quench_module_errors__"),
            ) {
                if let Some(Value::String(reason)) = errors.borrow().get(&module) {
                    let (value, error) =
                        crate::value::error::create_js_error_with_type(&reason, "SyntaxError");
                    crate::value::set_thrown_value(value);
                    return Err(error);
                }
            }
            self.env.borrow_mut().define(
                "__import_meta__".to_string(),
                Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))),
            );
            // Module code: `this` is undefined (ThisMode::module per ES spec)
            interpreter::set_this_binding(&self.env, Value::Undefined);
            let previous_strict = interpreter::is_strict_mode();
            interpreter::set_strict_mode(true);
            self.env.borrow_mut().define(
                "__quench_current_module_evaluating__".to_string(),
                Value::Boolean(true),
            );
            let result = interpreter::eval_program(&program, &mut self.env, Some(source), false);
            self.env.borrow_mut().define(
                "__quench_current_module_evaluating__".to_string(),
                Value::Boolean(false),
            );
            interpreter::set_strict_mode(previous_strict);
            result
        })();

        // Microtask checkpoint (see Context::eval)
        let microtask_result = crate::builtins::execute_pending_microtasks();

        // Clear thread-locals after eval completes
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = previous_context;
        });
        CURRENT_SOURCE.with(|cell| {
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

    /// Start a dynamic module import using this context's module environment.
    pub fn import_module(&mut self, source: &str) -> Result<Value, JsError> {
        crate::eval::statement::dynamic_import(source, &self.env, None, false, true)
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
        let previous_context = CURRENT_CONTEXT.with(|cell| {
            let previous = *cell.borrow();
            *cell.borrow_mut() = Some(ctx_ptr);
            previous
        });
        CURRENT_SOURCE.with(|cell| {
            *cell.borrow_mut() = Some(unsafe { std::mem::transmute::<&str, &str>(source) });
        });
        let result = (|| {
            let program = self.parse_typescript(source)?;
            // Script code: set `this = globalThis`
            interpreter::eval_program(&program, &mut self.env, Some(source), true)
        })();
        // Microtask checkpoint (see Context::eval)
        let microtask_result = crate::builtins::execute_pending_microtasks();
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = previous_context;
        });
        CURRENT_SOURCE.with(|cell| {
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
        let length = match name {
            "parseInt" => 2.0,
            "parseFloat" | "isNaN" | "isFinite" | "encodeURI" | "encodeURIComponent"
            | "decodeURI" | "decodeURIComponent" => 1.0,
            _ => 0.0,
        };
        nf.define_property(
            "length",
            Value::Number(length),
            crate::value::PropertyFlags {
                value: Some(Value::Number(length)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        // Per ES §17, every built-in Function object that is not anonymous has
        // a name property with attributes {[[Writable]]: false, [[Enumerable]]:
        // false, [[Configurable]]: true}. The propertyHelper.js harness asserts
        // this with `obj.hasOwnProperty('name')`, so the name must live in
        // own properties (the `nf.name` field is not exposed as own).
        nf.define_property(
            "name",
            Value::String(name.to_string()),
            crate::value::PropertyFlags {
                value: Some(Value::String(name.to_string())),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
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
