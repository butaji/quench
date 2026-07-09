//! Runtime context for the JavaScript engine.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::cell::RefCell;

use crate::arena::Arena;
use crate::interner::StringInterner;
use crate::shape::ShapeInterner;
use crate::value::{Value, JsError, Object, ObjectKind, NativeFunction};
use crate::env::Environment;
use crate::ast;
use crate::host;
use crate::shadow;
use crate::swc_parse;
use crate::interpreter;
use crate::stack_machine;
use crate::eval;

pub mod microtask;
pub mod tests;

pub use microtask::MicrotaskQueue;

/// Runtime context - holds the execution environment and globals
pub struct Context {
    pub arena: Arena<Object>,
    pub shadow_arena: shadow::ShadowArena,
    env: Rc<RefCell<Environment>>,
    pub string_interner: StringInterner,
    pub shape_interner: ShapeInterner,
    pub microtasks: MicrotaskQueue,
}

impl Context {
    /// Create a new runtime context
    pub fn new() -> Result<Self, JsError> {
        interpreter::reset_depth();
        let env = Environment::new();
        let mut ctx = Context {
            arena: Arena::new(),
            shadow_arena: shadow::ShadowArena::new(),
            env: Rc::new(RefCell::new(env)),
            string_interner: StringInterner::new(),
            shape_interner: ShapeInterner::new(),
            microtasks: MicrotaskQueue::new(),
        };
        ctx.init_builtins()?;
        Ok(ctx)
    }

    /// Reset the context to a clean state (useful for testing)
    pub fn reset(&mut self) -> Result<(), JsError> {
        interpreter::reset_depth();
        self.env = Rc::new(RefCell::new(Environment::new()));
        self.arena = Arena::new();
        self.shadow_arena = shadow::ShadowArena::new();
        self.string_interner = StringInterner::new();
        self.shape_interner = ShapeInterner::new();
        self.microtasks = MicrotaskQueue::new();
        self.init_builtins()?;
        Ok(())
    }

    /// Initialize built-in globals and functions
    fn init_builtins(&mut self) -> Result<(), JsError> {
        host::register_builtin_functions(self);

        // CommonJS module compatibility
        let exports = Object::new(ObjectKind::Ordinary);
        let exports_rc = Rc::new(RefCell::new(exports));
        let module_obj = Object::new(ObjectKind::Ordinary);
        let module_obj = Rc::new(RefCell::new(module_obj));
        module_obj.borrow_mut().set("exports", Value::Object(Rc::clone(&exports_rc)));
        self.set_global("exports".to_string(), Value::Object(Rc::clone(&exports_rc)));
        self.set_global("module".to_string(), Value::Object(module_obj));

        // ES Module cache for import/export support
        let module_cache = Object::new(ObjectKind::Ordinary);
        let module_cache_rc = Rc::new(RefCell::new(module_cache));
        self.set_global("__quench_modules__".to_string(), Value::Object(Rc::clone(&module_cache_rc)));
        
        // Also expose on globalThis for ES module resolution
        let global_obj = self.get_global("globalThis")
            .and_then(|g| {
                if let Value::Object(o) = g { Some(o) } else { None }
            });
        if let Some(global_obj) = global_obj {
            global_obj.borrow_mut().set("__quench_modules__", Value::Object(Rc::clone(&module_cache_rc)));
        }

        // JavaScript globals - globalThis is the global object itself
        let global_obj = Object::new(ObjectKind::Global);
        let global_obj = Rc::new(RefCell::new(global_obj));
        self.set_global("globalThis".to_string(), Value::Object(Rc::clone(&global_obj)));
        self.set_global("undefined".to_string(), Value::Undefined);
        self.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
        self.set_global("NaN".to_string(), Value::Number(f64::NAN));

        Ok(())
    }

    /// Evaluate a JavaScript source string using the recursive interpreter.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();
        let program = self.parse(source)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Evaluate an ES module source string using the recursive interpreter.
    pub fn eval_es_module(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();
        let program = swc_parse::parse_es_module(source)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Evaluate a JavaScript source string using the explicit-stack interpreter.
    pub fn eval_stack_machine(&self, source: &str) -> Result<Value, JsError> {
        let program = self.parse(source)?;
        let mut env = std::rc::Rc::clone(&self.env);
        stack_machine::eval_program(&program, &mut env)
    }

    /// Parse JavaScript source into an AST using swc
    pub fn parse(&self, source: &str) -> Result<crate::ast::Program, JsError> {
        swc_parse::parse_swc(source)
    }

    /// Parse TypeScript/TSX source into an AST using swc (strips type annotations)
    pub fn parse_typescript(&self, source: &str) -> Result<crate::ast::Program, JsError> {
        swc_parse::parse_typescript(source)
    }

    /// Evaluate a TypeScript/TSX source string using the recursive interpreter.
    pub fn eval_typescript(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();
        let program = self.parse_typescript(source)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Evaluate source using the shadow-tree interpreter.
    pub fn eval_shadow(&mut self, source: &str, mode: shadow::ModuleMode) -> Result<Value, JsError> {
        let script = swc_parse::parse_swc_script(source)?;
        let bump = bumpalo::Bump::new();

        let bindings: HashMap<String, shadow::Binding> = {
            let mut builder = shadow::ShadowBuilder::new(
                &bump,
                &mut self.string_interner,
                &self.shape_interner,
            );
            builder.collect_type_map(&script);
            shadow::lower::collect_script_bindings(&mut builder, &script);
            builder.bindings.clone()
        };

        let mut next_local: u16 = bindings
            .values()
            .map(|b| match b {
                shadow::Binding::Local(slot) => slot + 1,
                _ => 0,
            })
            .max()
            .unwrap_or(0);

        let mut nodes: Vec<&shadow::ShadowNode> = Vec::new();
        for stmt in &script.body {
            nodes.push(shadow::lower::lower_shadow_stmt(
                &bump,
                &mut self.string_interner,
                &self.shape_interner,
                &bindings,
                &mut next_local,
                stmt,
            )?);
        }

        let root: &shadow::ShadowNode = if nodes.is_empty() {
            bump.alloc(shadow::ShadowNode::This)
        } else if nodes.len() == 1 {
            nodes[0]
        } else {
            bump.alloc(shadow::ShadowNode::Block(nodes))
        };

        let result = shadow::ShadowVm::new(&bump, self, mode).run(root)?;
        Ok(shadow::lower::jsvalue_to_value(result, &self.string_interner))
    }

    /// Enqueue a microtask (function to be called asynchronously)
    pub fn enqueue_microtask(&mut self, task: Value) {
        self.microtasks.enqueue(task);
    }

    /// Process all queued microtasks
    pub fn process_microtasks(&mut self) -> Result<Value, JsError> {
        use crate::eval::call_value_with_this;
        let mut last_result = Value::Undefined;
        while let Some(task) = self.microtasks.dequeue() {
            match task {
                Value::Function(ref f) => {
                    last_result = call_value_with_this(Value::Function(f.clone()), vec![], Value::Undefined)?;
                }
                Value::NativeFunction(ref f) => {
                    last_result = call_value_with_this(Value::NativeFunction(f.clone()), vec![], Value::Undefined)?;
                }
                _ => {}
            }
        }
        Ok(last_result)
    }

    /// Check if there are pending microtasks
    pub fn has_pending_microtasks(&self) -> bool {
        !self.microtasks.is_empty()
    }

    /// Set a global value in the root environment.
    pub fn set_global(&mut self, name: String, value: Value) {
        self.env.borrow_mut().define(name, value);
    }

    /// Get a global value from the root environment.
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.env.borrow().get(name)
    }

    /// Get the inner environment (used by stack machine tests).
    #[allow(dead_code)]
    pub(crate) fn env(&self) -> &Rc<RefCell<Environment>> {
        &self.env
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
                let call_env_rc = Rc::new(RefCell::new(call_env));

                for (i, param) in params.iter().enumerate() {
                    let arg = args.get(i).cloned();
                    let value = match arg {
                        Some(Value::Undefined) if param.default.is_some() => {
                            eval::eval_expression(&param.default.as_ref().unwrap(), &call_env_rc)?
                        }
                        Some(v) => v,
                        None if param.default.is_some() => {
                            eval::eval_expression(&param.default.as_ref().unwrap(), &call_env_rc)?
                        }
                        None => Value::Undefined,
                    };
                    call_env_rc.borrow_mut().declare(param.name.clone(), value);
                }

                let call_env = call_env_rc;

                if f.is_arrow {
                    if let Some(arrow_body) = f.arrow_body.as_ref() {
                        match arrow_body {
                            ast::ArrowBody::Expression(expr) => {
                                eval::eval_expression(expr, &call_env)
                            }
                            ast::ArrowBody::Block(stmts) => {
                                eval::eval_statements(stmts, &call_env, true)
                            }
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                } else {
                    eval::eval_statements(&f.body, &call_env, false)
                }
            }
            Value::NativeFunction(nf) => nf.call(args),
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

    /// Load runtime.js from a path using the stack machine.
    pub fn load_runtime_from(&mut self, path: &Path) -> Result<(), JsError> {
        if path.exists() {
            let source = fs::read_to_string(path)
                .map_err(|e| JsError(format!("Failed to read runtime.js: {}", e)))?;
            self.eval_stack_machine(&source)?;
        }
        Ok(())
    }

    /// Register a module's exports for ES module import resolution.
    /// This is useful for testing ES modules without a file system.
    pub fn register_module(&mut self, path: &str, exports: Object) {
        let cache = self.get_global("__quench_modules__");
        if let Some(Value::Object(cache_obj)) = cache {
            cache_obj.borrow_mut().set(path, Value::Object(Rc::new(RefCell::new(exports))));
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
