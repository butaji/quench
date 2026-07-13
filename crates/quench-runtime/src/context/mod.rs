//! Runtime context for the JavaScript engine.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

thread_local! {
    pub static CURRENT_CONTEXT: RefCell<Option<*mut Context>> = const { RefCell::new(None) };
}

use crate::ast;
use crate::env::Environment;
use crate::eval;
use crate::host;
use crate::interner::StringInterner;
use crate::interpreter;
use crate::parser;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};

pub mod tests;

/// eval function implementation - executes JavaScript code in the current context.
/// Per ES spec §19.2.1, eval code inherits strict mode from its calling context.
/// We check for legacy octals here (before parsing the eval string) so that
/// eval in strict mode throws even when the eval string itself has no
/// "use strict" directive.
fn eval_impl(args: Vec<Value>, ctx: &mut Context) -> Result<Value, JsError> {
    let source = args
        .first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    if source.is_empty() {
        return Ok(Value::Undefined);
    }

    // Check for legacy octal BEFORE parsing, using inherited strict mode from
    // the outer context (is_strict_mode returns true if the code calling eval
    // is itself strict, e.g. "use strict"; eval("01;")).
    // Capture strict mode BEFORE calling ctx.eval() since ctx.eval -> eval_program
    // will modify strict mode (setting from directive, then restoring).
    let strict_inherited = crate::interpreter::is_strict_mode();
    let has_octal = crate::interpreter::has_legacy_octal(&source);
    if strict_inherited && has_octal {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "legacy octal literals are not allowed in strict mode",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    reject_eval_var_lexical_conflict(&source, ctx)?;

    // eval_impl is called from within ctx.eval(), which set CURRENT_CONTEXT.
    // We need to re-set it (and restore afterward) so that the test's second
    // (and third, ...) eval() call still has a valid context pointer.
    let ctx_ptr: *mut Context = ctx;
    let prev_ctx = CURRENT_CONTEXT.with(|cell| {
        let prev = cell.borrow();
        *prev
    });
    CURRENT_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(ctx_ptr);
    });
    // Attempt to parse first so a syntax error becomes a catchable SyntaxError
    // (ES §19.2.1.1 step 6). Parse errors carry no thrown value, and a stale
    // thrown value from earlier code must not be mistaken for the eval result.
    if let Err(e) = ctx.parse(&source) {
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = prev_ctx;
        });
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(&e.0, "SyntaxError");
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let result = ctx.eval(&source);
    CURRENT_CONTEXT.with(|cell| {
        *cell.borrow_mut() = prev_ctx;
    });
    match result {
        // eval threw a JS exception (SyntaxError, etc.) — propagate it.
        // eval_program sets the thrown value before returning Err.
        // We peek at it (don't consume) so eval_try_catch can also retrieve it.
        Err(_) => {
            if let Some(thrown) = crate::value::get_thrown_value() {
                let msg = crate::value::to_js_string(&thrown);
                Err(JsError(msg))
            } else {
                Err(JsError("unknown eval error".to_string()))
            }
        }
        Ok(v) => Ok(v),
    }
}

fn reject_eval_var_lexical_conflict(source: &str, ctx: &Context) -> Result<(), JsError> {
    let Ok(program) = ctx.parse(source) else {
        return Ok(());
    };
    let ast::Program::Script(body) = program;
    let mut names = Vec::new();
    crate::interpreter::collect_var_names_recursive(&body, &mut names);
    let eval_env =
        crate::interpreter::get_current_eval_env().unwrap_or_else(|| Rc::clone(&ctx.env));
    for name in names {
        if matches!(
            eval_env.borrow().get_kind(&name),
            Some(ast::VarKind::Let | ast::VarKind::Const)
        ) {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                &format!("Identifier '{}' has already been declared", name),
                "SyntaxError",
            );
            crate::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    Ok(())
}

/// Runtime context - holds the execution environment and globals
pub struct Context {
    env: Rc<RefCell<Environment>>,
    pub string_interner: StringInterner,
}

impl Context {
    /// Create a new runtime context
    pub fn new() -> Result<Self, JsError> {
        interpreter::reset_depth();
        let env = Environment::new();
        let mut ctx = Context {
            env: Rc::new(RefCell::new(env)),
            string_interner: StringInterner::new(),
        };

        // Set thread-local before init_builtins so eval function can access context
        let ctx_ptr: *mut Context = &mut ctx;
        CURRENT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx_ptr);
        });

        ctx.init_builtins()?;

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
        self.string_interner = StringInterner::new();
        // Clear the stale Promise prototype before it is rebuilt by init_builtins
        crate::builtins::promise::clear_promise_proto();
        // Reset global symbol registry for new realm
        crate::builtins::symbol::reset_global_symbol_registry();
        self.init_builtins()?;
        Ok(())
    }

    /// Initialize built-in globals and functions
    fn init_builtins(&mut self) -> Result<(), JsError> {
        host::register_builtin_functions(self);
        self.init_commonjs()?;
        self.init_es_module_cache()?;
        self.init_js_globals()?;
        self.register_eval_function()?;
        Ok(())
    }

    fn init_commonjs(&mut self) -> Result<(), JsError> {
        let exports = Object::new(ObjectKind::Ordinary);
        let exports_rc = Rc::new(RefCell::new(exports));
        let module_obj = Object::new(ObjectKind::Ordinary);
        let module_obj = Rc::new(RefCell::new(module_obj));
        module_obj
            .borrow_mut()
            .set("exports", Value::Object(Rc::clone(&exports_rc)));
        self.set_global("exports".to_string(), Value::Object(Rc::clone(&exports_rc)));
        self.set_global("module".to_string(), Value::Object(module_obj));
        Ok(())
    }

    fn init_es_module_cache(&mut self) -> Result<(), JsError> {
        let module_cache = Object::new(ObjectKind::Ordinary);
        let module_cache_rc = Rc::new(RefCell::new(module_cache));
        self.set_global(
            "__quench_modules__".to_string(),
            Value::Object(Rc::clone(&module_cache_rc)),
        );
        if let Some(Value::Object(global_obj)) = self.get_global("globalThis") {
            global_obj.borrow_mut().set(
                "__quench_modules__",
                Value::Object(Rc::clone(&module_cache_rc)),
            );
        }
        Ok(())
    }

    fn init_js_globals(&mut self) -> Result<(), JsError> {
        let global_obj = Object::new(ObjectKind::Global);
        let global_obj = Rc::new(RefCell::new(global_obj));
        // Mark globalThis itself as writable, non-enumerable, configurable so
        // property-descriptor.js's verifyProperty(this, "globalThis", {...})
        // passes (the verifyProperty helper uses getOwnPropertyDescriptor).
        global_obj.borrow_mut().define(
            "globalThis",
            Value::Object(Rc::clone(&global_obj)),
            crate::value::PropertyFlags {
                value: Some(Value::Object(Rc::clone(&global_obj))),
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
        self.set_global(
            "globalThis".to_string(),
            Value::Object(Rc::clone(&global_obj)),
        );

        // Spec-mandated value properties of the global object: non-writable,
        // non-enumerable, non-configurable. assign_to_identifier checks these
        // descriptors in strict mode and throws TypeError when assigning.
        let value_flags = crate::value::PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: false,
        };
        let mut define_value_prop = |key: &str, val: Value, global_obj: &Rc<RefCell<Object>>| {
            let mut flags = value_flags.clone();
            flags.value = Some(val.clone());
            global_obj.borrow_mut().define(key, val, flags);
        };
        define_value_prop("undefined", Value::Undefined, &global_obj);
        define_value_prop("Infinity", Value::Number(f64::INFINITY), &global_obj);
        define_value_prop("NaN", Value::Number(f64::NAN), &global_obj);

        self.set_global("undefined".to_string(), Value::Undefined);
        self.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
        self.set_global("NaN".to_string(), Value::Number(f64::NAN));
        Ok(())
    }

    /// Register the eval function as a global
    fn register_eval_function(&mut self) -> Result<(), JsError> {
        let eval_fn = NativeFunction::new(|args: Vec<Value>| {
            // Get context from thread-local at call time (not at registration time)
            // This avoids UB from storing a raw pointer that becomes invalid after
            // Context::new() returns.
            let ctx_ptr =
                CURRENT_CONTEXT.with(|cell| cell.borrow().unwrap_or_else(std::ptr::null_mut));
            if ctx_ptr.is_null() {
                return Err(JsError("eval called outside of context".to_string()));
            }
            // SAFETY: Thread-local is set by Context::eval() before any code runs,
            // ensuring ctx_ptr is valid. Cleared after eval completes.
            let ctx = unsafe { &mut *ctx_ptr };
            eval_impl(args, ctx)
        });

        self.set_global("eval".to_string(), Value::NativeFunction(Rc::new(eval_fn)));
        Ok(())
    }
}

impl Context {
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
            interpreter::eval_program(&program, &mut self.env, Some(source))
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
            interpreter::eval_program(&program, &mut self.env, Some(source))
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
            interpreter::eval_program(&program, &mut self.env, Some(source))
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
    pub fn set_global(&mut self, name: String, value: Value) {
        self.env.borrow_mut().define(name, value);
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
        self.set_global(
            name.to_string(),
            Value::NativeFunction(Rc::new(NativeFunction::new(f))),
        );
    }

    /// Call a global function with arguments
    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, JsError> {
        let func = self
            .get_global(name)
            .ok_or_else(|| JsError(format!("Function not found: {}", name)))?;

        match func {
            Value::Function(f) => self.call_js_function(&f, args),
            Value::NativeFunction(nf) => nf.call(Value::Undefined, args),
            _ => Err(JsError(format!("{} is not a function", name))),
        }
    }

    fn call_js_function(
        &mut self,
        f: &crate::value::ValueFunction,
        args: Vec<Value>,
    ) -> Result<Value, JsError> {
        let closure = Rc::clone(&f.closure);
        let call_env_rc = Rc::new(RefCell::new(Environment::with_parent(closure)));
        Self::bind_params(&f.params, &args, &call_env_rc, f.is_arrow)?;

        if f.is_arrow {
            Self::eval_arrow_body(&f.arrow_body, &call_env_rc)
        } else {
            eval::eval_function_body(&f.body, &call_env_rc, false)
        }
    }

    fn bind_params(
        params: &[ast::Param],
        args: &[Value],
        call_env: &Rc<RefCell<Environment>>,
        is_arrow: bool,
    ) -> Result<(), JsError> {
        for (i, param) in params.iter().enumerate() {
            let value = Self::resolve_param_value(param, args, i, call_env, is_arrow)?;
            call_env.borrow_mut().declare(param.name.clone(), value);
        }
        Ok(())
    }

    fn resolve_param_value(
        param: &ast::Param,
        args: &[Value],
        index: usize,
        call_env: &Rc<RefCell<Environment>>,
        is_arrow: bool,
    ) -> Result<Value, JsError> {
        match args.get(index).cloned() {
            Some(Value::Undefined) if param.default.is_some() => {
                eval::eval_expression(param.default.as_ref().unwrap(), call_env, is_arrow)
            }
            Some(v) => Ok(v),
            None if param.default.is_some() => {
                eval::eval_expression(param.default.as_ref().unwrap(), call_env, is_arrow)
            }
            None => Ok(Value::Undefined),
        }
    }

    fn eval_arrow_body(
        arrow_body: &Option<ast::ArrowBody>,
        call_env: &Rc<RefCell<Environment>>,
    ) -> Result<Value, JsError> {
        match arrow_body {
            Some(ast::ArrowBody::Expression(expr)) => eval::eval_expression(expr, call_env, true),
            Some(ast::ArrowBody::Block(stmts)) => eval::eval_function_body(stmts, call_env, true),
            None => Ok(Value::Undefined),
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
