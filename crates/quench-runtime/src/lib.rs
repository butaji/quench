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
pub mod env;
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

use std::cell::Cell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::cell::RefCell;

use crate::arena::Arena;
use crate::interner::StringInterner;
use crate::shape::ShapeInterner;

pub use value::{Value, JsError};
pub use ast::Program;
pub use host::{HostFunctions, register_native};

// Re-export commonly used types
pub use value::{Object, ObjectKind, ValueFunction, NativeFunction};
pub use env::Environment;

/// Runtime context - holds the execution environment and globals
pub struct Context {
    pub arena: Arena<Object>,
    pub shadow_arena: shadow::ShadowArena,
    env: Rc<RefCell<Environment>>,
    pub string_interner: StringInterner,
    pub shape_interner: ShapeInterner,
}

impl Context {
    /// Create a new runtime context
    pub fn new() -> Result<Self, JsError> {
        interpreter::reset_depth(); // Reset depth for new context
        let env = Environment::new();
        let mut ctx = Context {
            arena: Arena::new(),
            shadow_arena: shadow::ShadowArena::new(),
            env: Rc::new(RefCell::new(env)),
            string_interner: StringInterner::new(),
            shape_interner: ShapeInterner::new(),
        };
        ctx.init_builtins()?;
        Ok(ctx)
    }

    /// Reset the context to a clean state (useful for testing)
    pub fn reset(&mut self) -> Result<(), JsError> {
        interpreter::reset_depth(); // Reset depth counter
        self.env = Rc::new(RefCell::new(Environment::new()));
        self.arena = Arena::new();
        self.shadow_arena = shadow::ShadowArena::new();
        self.string_interner = StringInterner::new();
        self.shape_interner = ShapeInterner::new();
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
    pub fn parse(&self, source: &str) -> Result<Program, JsError> {
        swc_parse::parse_swc(source)
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
            collect_script_bindings(&mut builder, &script);
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
            nodes.push(lower_shadow_stmt(
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
        Ok(jsvalue_to_value(result, &self.string_interner))
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
                                interpreter::eval_expression(expr, &call_env)
                            }
                            ast::ArrowBody::Block(stmts) => {
                                interpreter::eval_statements(&*stmts, &call_env, true)
                            }
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                } else {
                    interpreter::eval_statements(&*f.body, &call_env, false)
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

/// Collect `var`/`let`/`const` bindings declared at the top level of a script
/// and assign each one a local slot in the shadow builder.
fn collect_script_bindings<'a>(
    builder: &mut shadow::ShadowBuilder<'a>,
    script: &swc_ecma_ast::Script,
) {
    for stmt in &script.body {
        if let swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::Var(var_decl)) = stmt {
            for decl in &var_decl.decls {
                if let swc_ecma_ast::Pat::Ident(ident) = &decl.name {
                    let name = ident.id.sym.to_string();
                    let slot = builder.local_for(&name);
                    let _sym = builder.intern(&name);
                    builder.bindings.insert(name, shadow::Binding::Local(slot));
                }
            }
        }
    }
}

/// Lower a single swc statement into a shadow tree node.
fn lower_shadow_stmt<'bump>(
    bump: &'bump bumpalo::Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, shadow::Binding>,
    next_local: &mut u16,
    stmt: &swc_ecma_ast::Stmt,
) -> Result<&'bump shadow::ShadowNode<'bump>, JsError> {
    match stmt {
        swc_ecma_ast::Stmt::Expr(expr_stmt) => {
            lower_shadow_expr(bump, interner, shapes, bindings, next_local, &expr_stmt.expr)
        }
        swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::Var(var_decl)) => {
            let mut last = None;
            for decl in &var_decl.decls {
                if let swc_ecma_ast::Pat::Ident(ident) = &decl.name {
                    let name = ident.id.sym.to_string();
                    let slot = var_slot(&name, bindings, next_local);
                    if let Some(init) = &decl.init {
                        let value = lower_shadow_expr(bump, interner, shapes, bindings, next_local, init)?;
                        let node = bump.alloc(shadow::ShadowNode::StoreLocal {
                            index: slot,
                            value,
                        });
                        last = Some(node);
                    }
                }
            }
            Ok(last.unwrap_or_else(|| bump.alloc(shadow::ShadowNode::This)))
        }
        _ => Err(JsError(format!("unsupported shadow statement: {:?}", stmt))),
    }
}

/// Look up the local slot for a declared variable, allocating a fresh slot if
/// the binding has not been collected.
fn var_slot(
    name: &str,
    bindings: &HashMap<String, shadow::Binding>,
    next_local: &mut u16,
) -> u16 {
    if let Some(shadow::Binding::Local(slot)) = bindings.get(name) {
        *slot
    } else {
        let slot = *next_local;
        *next_local += 1;
        slot
    }
}

/// Lower a single swc expression into a shadow tree node.
fn lower_shadow_expr<'bump>(
    bump: &'bump bumpalo::Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, shadow::Binding>,
    next_local: &mut u16,
    expr: &swc_ecma_ast::Expr,
) -> Result<&'bump shadow::ShadowNode<'bump>, JsError> {
    match expr {
        swc_ecma_ast::Expr::Bin(bin) => {
            let left = lower_shadow_expr(bump, interner, shapes, bindings, next_local, &bin.left)?;
            let right = lower_shadow_expr(bump, interner, shapes, bindings, next_local, &bin.right)?;
            let node = match bin.op {
                swc_ecma_ast::BinaryOp::Add => shadow::ShadowNode::Add {
                    left,
                    right,
                    state: Cell::new(shadow::AddState::Uninitialized),
                    hint: shadow::TypeHint::Any,
                },
                swc_ecma_ast::BinaryOp::Sub => shadow::ShadowNode::Sub { left, right },
                swc_ecma_ast::BinaryOp::Mul => shadow::ShadowNode::Mul { left, right },
                swc_ecma_ast::BinaryOp::Div => shadow::ShadowNode::Div { left, right },
                _ => return Err(JsError(format!("unsupported binary operator: {:?}", bin.op))),
            };
            Ok(bump.alloc(node))
        }
        swc_ecma_ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(binding) = bindings.get(&name) {
                Ok(bump.alloc(shadow::ShadowNode::BindingRead(*binding)))
            } else {
                let sym = interner.intern(&name);
                Ok(bump.alloc(shadow::ShadowNode::GlobalRead(sym)))
            }
        }
        swc_ecma_ast::Expr::Lit(lit) => match lit {
            swc_ecma_ast::Lit::Num(num) => {
                let v = num.value;
                if v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64 {
                    Ok(bump.alloc(shadow::ShadowNode::LiteralInt(v as i32)))
                } else {
                    Ok(bump.alloc(shadow::ShadowNode::LiteralDouble(v)))
                }
            }
            swc_ecma_ast::Lit::Str(s) => {
                let sym = interner.intern(s.value.as_str().unwrap_or(""));
                Ok(bump.alloc(shadow::ShadowNode::LiteralString(sym)))
            }
            swc_ecma_ast::Lit::Bool(b) => {
                Ok(bump.alloc(shadow::ShadowNode::LiteralInt(if b.value { 1 } else { 0 })))
            }
            swc_ecma_ast::Lit::Null(_) => Ok(bump.alloc(shadow::ShadowNode::This)),
            _ => Err(JsError(format!("unsupported literal: {:?}", lit))),
        },
        swc_ecma_ast::Expr::Object(obj_lit) => {
            let mut prop_names = Vec::new();
            let mut prop_values: Vec<(crate::interner::Symbol, &swc_ecma_ast::Expr)> = Vec::new();
            for prop in &obj_lit.props {
                match prop {
                    swc_ecma_ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                        swc_ecma_ast::Prop::KeyValue(kv) => {
                            let name = match &kv.key {
                                swc_ecma_ast::PropName::Ident(ident) => ident.sym.to_string(),
                                swc_ecma_ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                _ => return Err(JsError("unsupported object key".into())),
                            };
                            let sym = interner.intern(&name);
                            prop_names.push(sym);
                            prop_values.push((sym, &kv.value));
                        }
                        _ => return Err(JsError("unsupported object property".into())),
                    },
                    _ => return Err(JsError("unsupported spread property".into())),
                }
            }
            let shape = shapes.shape_for(&prop_names);
            let mut node: &shadow::ShadowNode = bump.alloc(shadow::ShadowNode::NewObject {
                shape: shape.clone(),
            });
            for (sym, value_expr) in prop_values {
                let value = lower_shadow_expr(bump, interner, shapes, bindings, next_local, value_expr)?;
                let offset = shape
                    .find_offset(sym)
                    .ok_or_else(|| JsError("shape missing property".into()))?;
                let is_inline = offset < shadow::INLINE_SLOTS;
                let store_offset = if is_inline { offset } else { offset - shadow::INLINE_SLOTS };
                node = bump.alloc(shadow::ShadowNode::StaticPropWrite {
                    obj: node,
                    prop: sym,
                    shape_id: shape.id,
                    offset: store_offset as u16,
                    is_inline,
                    value,
                });
            }
            Ok(node)
        }
        swc_ecma_ast::Expr::Member(member) => {
            let obj = lower_shadow_expr(bump, interner, shapes, bindings, next_local, &member.obj)?;
            let prop_name = match &member.prop {
                swc_ecma_ast::MemberProp::Ident(ident) => ident.sym.to_string(),
                _ => return Err(JsError("unsupported member property".into())),
            };
            let sym = interner.intern(&prop_name);
            Ok(bump.alloc(shadow::ShadowNode::PropRead {
                obj,
                prop: sym,
                cache: Cell::new(shadow::PropCache::default()),
            }))
        }
        _ => Err(JsError(format!("unsupported shadow expression: {:?}", expr))),
    }
}

fn jsvalue_to_value(
    js: crate::nanbox::JSValue,
    interner: &crate::interner::StringInterner,
) -> Value {
    if js.is_undefined() { Value::Undefined }
    else if js.is_null() { Value::Null }
    else if js.is_true() { Value::Boolean(true) }
    else if js.is_false() { Value::Boolean(false) }
    else if js.is_int32() { Value::Number(js.as_int32_unchecked() as f64) }
    else if js.is_double() { Value::Number(js.as_double_unchecked()) }
    else if js.is_object() {
        Value::ObjectId(js.as_object().expect("nanbox claimed object"))
    }
    else if js.is_string() {
        let sym = js.as_string().unwrap();
        let s = interner.resolve(sym).unwrap_or("").to_string();
        Value::String(s)
    } else if js.is_symbol() {
        let sym = js.as_symbol().unwrap();
        let s = interner.resolve(sym).unwrap_or("").to_string();
        Value::Symbol(s)
    } else {
        Value::Undefined
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
}
