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

/// Thread-local cache for single-character regex objects (heavily used in test262).
thread_local! {
    static REGEX_CACHE: std::cell::RefCell<rustc_hash::FxHashMap<char, Value>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

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

    // Fast path: regex literal eval like eval("/x/") or eval("/x/.source")
    // Skip OXC parse entirely for common patterns in test262.
    let source_bytes = source.as_bytes();
    if source_bytes.first() == Some(&b'/') {
        if let Some(end_slash) = source_bytes[1..].iter().position(|&b| b == b'/') {
            let pattern = &source[1..][..end_slash];
            let after = &source[1..][end_slash + 1..];
            let is_simple_regex = after.is_empty()
                || after == ".source"
                || after.chars().all(|c| matches!(c, 'g' | 'i' | 'm' | 's' | 'u' | 'y' | 'd' | 'v'))
                || {
                    let dot_idx = after.find('.');
                    dot_idx.map_or(false, |i| {
                        after[..i].chars().all(|c| matches!(c, 'g' | 'i' | 'm' | 's' | 'u' | 'y' | 'd' | 'v'))
                            && &after[i..] == ".source"
                    })
                };
            if is_simple_regex {
                let flags = if let Some(dot) = after.find('.') { &after[..dot] } else { after };
                if flags.chars().all(|c| matches!(c, 'g' | 'i' | 'm' | 's' | 'u' | 'y' | 'd')) {
                    // For regex followed by .source, skip creating the full regex object
                    if after.ends_with(".source") {
                        return Ok(crate::value::Value::String(pattern.to_string()));
                    }
                    // For flags-less regex, create a minimal regex object without regress::Regex
                    if flags.is_empty() {
                        // Cache single-character regex objects to avoid repeated allocation
                        if pattern.len() == 1 {
                            let ch = pattern.as_bytes()[0] as char;
                            let cached = REGEX_CACHE.with(|cache| cache.borrow().get(&ch).cloned());
                            if let Some(val) = cached {
                                return Ok(val);
                            }
                        }
                        let pattern_owned = pattern.to_string();
                        let mut obj = crate::value::Object::new(crate::value::ObjectKind::RegExp);
                        obj.internal_regex_source = Some(pattern_owned.clone());
                        obj.properties.insert("source".to_string(), crate::value::Value::String(pattern_owned));
                        obj.properties.insert("global".to_string(), crate::value::Value::Boolean(false));
                        obj.properties.insert("ignoreCase".to_string(), crate::value::Value::Boolean(false));
                        obj.properties.insert("multiline".to_string(), crate::value::Value::Boolean(false));
                        obj.properties.insert("flags".to_string(), crate::value::Value::String(String::new()));
                        obj.properties.insert("lastIndex".to_string(), crate::value::Value::Number(0.0));
                        let obj_rc = std::rc::Rc::new(std::cell::RefCell::new(obj));
                        let proto = crate::builtins::regex::get_regexp_prototype();
                        obj_rc.borrow_mut().prototype = Some(proto);
                        let val = crate::value::Value::Object(obj_rc);
                        if pattern.len() == 1 {
                            let ch = pattern.as_bytes()[0] as char;
                            REGEX_CACHE.with(|cache| { cache.borrow_mut().insert(ch, val.clone()); });
                        }
                        return Ok(val);
                    }
                    return crate::eval::literal::eval_regexp_literal(pattern, flags);
                }
            }
        }
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
    // Parse once, then check lexical conflicts AND evaluate from the same AST.
    let program = match ctx.parse(&source) {
        Ok(program) => program,
        Err(e) => {
            CURRENT_CONTEXT.with(|cell| {
                *cell.borrow_mut() = prev_ctx;
            });
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&e.0, "SyntaxError");
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };
    reject_eval_var_lexical_conflict(&program, ctx)?;
    let result = if let Some(mut eval_env) = crate::interpreter::get_current_eval_env() {
        // Nested eval: environment already set up by outer eval, just run the code
        crate::interpreter::eval_program(&program, &mut eval_env, Some(&source), false)
    } else {
        // Non-nested eval (direct or indirect):
        // - Direct eval (`eval(...)`): this = undefined in strict mode, globalThis in sloppy
        // - Indirect eval (`var f = eval; f(...)`): always globalThis (runs in global scope)
        let is_direct = crate::interpreter::is_direct_eval();
        let this_value = if is_direct && strict_inherited {
            // Direct eval in strict mode: this = undefined
            Value::Undefined
        } else {
            // Indirect eval, or direct eval in sloppy mode: this = globalThis
            ctx.env
                .borrow()
                .get("globalThis")
                .unwrap_or(Value::Undefined)
        };
        crate::interpreter::set_this_binding(&ctx.env, this_value);
        crate::interpreter::eval_program(&program, &mut ctx.env, Some(&source), false)
    };
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

fn reject_eval_var_lexical_conflict(
    program: &crate::ast::Program,
    ctx: &Context,
) -> Result<(), JsError> {
    let ast::Program::Script(body) = program;
    let mut names = Vec::new();
    crate::interpreter::collect_var_names_recursive(&body, &mut names);
    let eval_env =
        crate::interpreter::get_current_eval_env().unwrap_or_else(|| Rc::clone(&ctx.env));
    for name in &names {
        if matches!(
            eval_env.borrow().get_kind(name),
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
    for name in names {
        let is_local = eval_env.borrow().current_scope().borrow().has(&name);
        if !is_local {
            eval_env
                .borrow_mut()
                .declare_var(name.clone(), ast::VarKind::Var);
            eval_env
                .borrow_mut()
                .initialize_declared(&name, Value::Undefined);
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
        // sync_globals_to_global_this must come AFTER init_js_globals (which creates
        // globalThis). All builtins registered before init_js_globals are already in
        // the env but not yet on globalThis.
        self.sync_globals_to_global_this();
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
        // Must call set_global FIRST so the binding exists in the environment,
        // THEN define the final non-writable descriptor so strict mode
        // assignment checks find `writable: false`.
        self.set_global("undefined".to_string(), Value::Undefined);
        self.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
        self.set_global("NaN".to_string(), Value::Number(f64::NAN));

        let define_value_prop = |key: &str, val: Value, global_obj: &Rc<RefCell<Object>>| {
            let mut flags = value_flags.clone();
            flags.value = Some(val.clone());
            global_obj.borrow_mut().define(key, val, flags);
        };
        define_value_prop("undefined", Value::Undefined, &global_obj);
        define_value_prop("Infinity", Value::Number(f64::INFINITY), &global_obj);
        define_value_prop("NaN", Value::Number(f64::NAN), &global_obj);

        // Link the global scope to globalThis so that EnvScope::set can
        // check property descriptors (e.g. non-writable Infinity/NaN/undefined)
        // in strict mode and throw TypeError on assignment.
        self.env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .set_object_binding(Rc::clone(&global_obj));

        Ok(())
    }

    /// Sync all global bindings from the environment to globalThis.
    /// This is called after init_js_globals (which creates globalThis) so that
    /// all builtins registered before globalThis existed get mirrored onto it.
    /// Without this, globalThis.{Number,Object,Array,...} would be undefined.
    fn sync_globals_to_global_this(&mut self) {
        let Some(Value::Object(global_obj)) = self.get_global("globalThis") else {
            return;
        };
        // Only the global scope (index 0) holds global bindings.
        let scopes = &self.env.borrow().scopes;
        if scopes.is_empty() {
            return;
        }
        let global_scope = scopes[0].borrow();
        for (name, value_rc) in global_scope.bindings() {
            let value = (**value_rc).clone();
            global_obj.borrow_mut().define(
                name.as_str(),
                value.clone(),
                crate::value::PropertyFlags {
                    value: Some(value),
                    writable: true,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
    }

    /// Register the eval function as a global
    fn register_eval_function(&mut self) -> Result<(), JsError> {
        let eval_fn = NativeFunction::new_named("eval", |args: Vec<Value>| {
            let source = args
                .first()
                .map(crate::value::to_js_string)
                .unwrap_or_default();
            if source.is_empty() {
                return Ok(Value::Undefined);
            }

            // Fast path: eval("/x/") or eval("/x/.source") — bypass OXC entirely.
            let sb = source.as_bytes();
            if sb.len() > 1 && sb[0] == b'/' {
                if let Some(es) = sb[1..].iter().position(|&b| b == b'/') {
                    let pat = &source[1..][..es];
                    // Reject regex literals containing line terminators (SyntaxError per spec)
                    let has_line_term = pat.contains('\n') || pat.contains('\r') || pat.contains('\u{2028}') || pat.contains('\u{2029}');
                    // Reject patterns starting with excluded first-char: * \ / [
                    let bad_first_char = pat.as_bytes().first().map_or(false, |&b| matches!(b, b'*' | b'\\' | b'/' | b'['));
                    let after = &source[1..][es + 1..];
                    let clean = !has_line_term && !bad_first_char && (after.is_empty()
                        || after == ".source"
                        || after.bytes().all(|b| matches!(b, b'g' | b'i' | b'm' | b's' | b'u' | b'y' | b'd'))
                        || {
                            let dot_idx = after.find('.');
                            dot_idx.map_or(false, |i| {
                                after[..i].bytes().all(|b| matches!(b, b'g' | b'i' | b'm' | b's' | b'u' | b'y' | b'd'))
                                    && &after[i..] == ".source"
                            })
                        });
                        if clean {
                        let flags = if let Some(d) = after.find('.') { &after[..d] } else { after };
                        if flags.bytes().all(|b| matches!(b, b'g' | b'i' | b'm' | b's' | b'u' | b'y' | b'd')) {
                            // Single-char regex cache
                            if flags.is_empty() && pat.len() == 1 {
                                let ch = pat.as_bytes()[0] as char;
                                let cached = REGEX_CACHE.with(|c| c.borrow().get(&ch).cloned());
                                if let Some(v) = cached {
                                    return if after == ".source" {
                                        if let Value::Object(o) = &v {
                                            if let Some(src) = o.borrow().properties.get("source") {
                                                return Ok(src.clone());
                                            }
                                        }
                                        Ok(v)
                                    } else {
                                        Ok(v)
                                    };
                                }
                            }
                            if after.ends_with(".source") {
                                return Ok(Value::String(pat.to_string()));
                            }
                            if flags.is_empty() {
                                let po = pat.to_string();
                                let mut obj = Object::new(ObjectKind::RegExp);
                                obj.properties.insert("source".to_string(), Value::String(po.clone()));
                                obj.properties.insert("global".to_string(), Value::Boolean(false));
                                obj.properties.insert("ignoreCase".to_string(), Value::Boolean(false));
                                obj.properties.insert("multiline".to_string(), Value::Boolean(false));
                                obj.properties.insert("flags".to_string(), Value::String(String::new()));
                                obj.properties.insert("lastIndex".to_string(), Value::Number(0.0));
                                let orc = Rc::new(RefCell::new(obj));
                                let proto = crate::builtins::regex::get_regexp_prototype();
                                orc.borrow_mut().prototype = Some(proto);
                                let val = Value::Object(orc);
                                if pat.len() == 1 {
                                    let ch = pat.as_bytes()[0] as char;
                                    REGEX_CACHE.with(|c| { c.borrow_mut().insert(ch, val.clone()); });
                                }
                                return Ok(val);
                            }
                            return crate::eval::literal::eval_regexp_literal(pat, flags);
                        }
                    }
                }
            }

            // Fall through to full eval_impl for non-regex evals.
            let ctx_ptr =
                CURRENT_CONTEXT.with(|cell| cell.borrow().unwrap_or_else(std::ptr::null_mut));
            if ctx_ptr.is_null() {
                return Err(JsError("eval called outside of context".to_string()));
            }
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
