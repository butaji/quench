//! Runtime context implementation for the JavaScript runtime.

use std::collections::{HashMap, VecDeque};
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

/// Microtask queue for Promise resolution
pub struct MicrotaskQueue {
    queue: VecDeque<Value>,
}

impl MicrotaskQueue {
    pub fn new() -> Self {
        MicrotaskQueue { queue: VecDeque::new() }
    }

    pub fn enqueue(&mut self, task: Value) {
        self.queue.push_back(task);
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn dequeue(&mut self) -> Option<Value> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

impl Default for MicrotaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

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

    /// Evaluate a JavaScript source string using the recursive interpreter.
    /// WARNING: May cause stack overflow with deep recursion.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth(); // Reset depth for each top-level eval
        let program = self.parse(source)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Evaluate a JavaScript source string using the explicit-stack interpreter.
    /// This prevents stack overflow with deep recursion but may have less features.
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
    /// Strips type annotations via swc and executes the resulting JavaScript.
    pub fn eval_typescript(&mut self, source: &str) -> Result<Value, JsError> {
        interpreter::reset_depth();
        let program = self.parse_typescript(source)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    /// Evaluate source using the shadow-tree interpreter.
    pub fn eval_shadow(&mut self, source: &str, mode: shadow::ModuleMode) -> Result<Value, JsError> {
        let script = swc_parse::parse_swc_script(source)?;
        let bump = bumpalo::Bump::new();

        // Build a ShadowBuilder to collect the TypeScript type map and script
        // bindings. The builder holds a mutable borrow of `string_interner`, so
        // it is confined to this inner scope; the bindings map is cloned out
        // before the builder is dropped.
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
    /// Returns the last result or Ok(()) if queue was empty
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
                
                for (i, param) in params.iter().enumerate() {
                    let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
                    call_env.declare(param.clone(), arg);
                }
                
                let call_env = Rc::new(RefCell::new(call_env));
                
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
            Value::NativeFunction(nf) => {
                nf.call(args)
            }
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

    #[test]
    fn test_deep_recursion_does_not_stack_overflow() {
        interpreter::reset_depth();
        let ctx = Context::new().unwrap();
        // A simple recursive function with depth 100 must not overflow the
        // native Rust stack. This test uses the stack machine to verify
        // explicit-stack execution works correctly.
        let source = r#"
            function recurse(n) {
                if (n <= 0) return 0;
                return 1 + recurse(n - 1);
            }
            recurse(100);
        "#;
        let program = ctx.parse(source).unwrap();
        let mut env = std::rc::Rc::clone(ctx.env());
        let result = stack_machine::eval_program(&program, &mut env);
        assert!(result.is_ok(), "deep recursion should not stack overflow: {:?}", result);
        assert_eq!(result.unwrap(), Value::Number(100.0));
    }

    #[test]
    fn test_function_declaration_overrides_existing_global() {
        let mut ctx = Context::new().unwrap();
        // First script defines a function.
        ctx.eval("function mountTree() { return 'runtime'; }").unwrap();
        // A later script should be able to override it with a new declaration.
        let result = ctx.eval(r#"
            function mountTree() { return 'user'; }
            mountTree();
        "#).unwrap();
        assert_eq!(result, Value::String("user".to_string()));
    }

    #[test]
    fn test_duplicate_function_declaration_last_wins() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            function f() { return 1; }
            function f() { return 2; }
            f();
        "#).unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn test_eval_shadow_simple_add() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval_shadow("1 + 2", crate::shadow::ModuleMode::Static).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_eval_shadow_var() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval_shadow("var x = 5; x + x", crate::shadow::ModuleMode::Static).unwrap();
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn test_eval_shadow_object_prop() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval_shadow("var o = {a: 3}; o.a", crate::shadow::ModuleMode::Static).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_runtime_ink_object() {
        let mut ctx = Context::new().unwrap();
        let runtime_path = std::path::Path::new(
            "/Users/admin/Code/GitHub/quench/src/runtime.js"
        );
        ctx.load_runtime_from(runtime_path).unwrap();
        
        // Test ink object exists
        let result = ctx.eval("typeof ink");
        assert!(result.is_ok(), "typeof ink failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::String("object".to_string()));
        
        // Test ink.createElement exists
        let result = ctx.eval("typeof ink.createElement");
        assert!(result.is_ok(), "typeof ink.createElement failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
        
        // Test ink.render exists
        let result = ctx.eval("typeof ink.render");
        assert!(result.is_ok(), "typeof ink.render failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
        
        // Test ink.Box
        let result = ctx.eval("ink.Box");
        assert!(result.is_ok(), "ink.Box failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::String("ink-box".to_string()));
        
        // Test ink.Text
        let result = ctx.eval("ink.Text");
        assert!(result.is_ok(), "ink.Text failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::String("ink-text".to_string()));
    }

    #[test]
    fn test_function_call_and_apply() {
        let mut ctx = Context::new().unwrap();
        let runtime_path = std::path::Path::new(
            "/Users/admin/Code/GitHub/quench/src/runtime.js"
        );
        ctx.load_runtime_from(runtime_path).unwrap();
        
        // Test Function.prototype.call with this binding
        let result = ctx.eval(r#"
            const obj = { x: 42 };
            const test = function() { return this.x; };
            test.call(obj);
        "#);
        assert_eq!(result.unwrap(), Value::Number(42.0));
        
        // Test Function.prototype.call without this binding
        let result = ctx.eval(r#"
            const test = function() { return 42; };
            test.call();
        "#);
        assert_eq!(result.unwrap(), Value::Number(42.0));
        
        // Test Function.prototype.apply
        let result = ctx.eval(r#"
            const obj = { x: 100 };
            const test = function() { return this.x; };
            test.apply(obj);
        "#);
        assert_eq!(result.unwrap(), Value::Number(100.0));
    }
    
    #[test]
    fn test_component_instance_render() {
        let mut ctx = Context::new().unwrap();
        let runtime_path = std::path::Path::new(
            "/Users/admin/Code/GitHub/quench/src/runtime.js"
        );
        
        // Load runtime using stack machine
        ctx.load_runtime_from(runtime_path).unwrap();
        
        // Check what ComponentInstance.prototype looks like
        let result = ctx.eval("typeof ComponentInstance.prototype");
        println!("prototype type: {:?}", result);
        
        // Now set testProp using stack machine
        let result = ctx.eval_stack_machine("ComponentInstance.prototype.testProp = 42");
        println!("stack machine set result: {:?}", result);
        
        // Check if testProp is set
        let result = ctx.eval("ComponentInstance.prototype.testProp");
        println!("testProp value: {:?}", result);
    }
}
