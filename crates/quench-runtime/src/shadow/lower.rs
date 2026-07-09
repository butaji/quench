//! Shadow tree lowering from swc AST.
//!
//! Converts swc AST nodes into shadow tree nodes for the SSTI interpreter.

use std::cell::Cell;
use std::collections::HashMap;

use bumpalo::Bump;

use crate::interner::StringInterner;
use crate::shape::ShapeInterner;
use crate::value::JsError;

/// Collect `var`/`let`/`const` bindings declared at the top level of a script
/// and assign each one a local slot in the shadow builder.
pub fn collect_script_bindings(
    builder: &mut crate::shadow::ShadowBuilder,
    script: &swc_ecma_ast::Script,
) {
    for stmt in &script.body {
        if let swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::Var(var_decl)) = stmt {
            for decl in &var_decl.decls {
                if let swc_ecma_ast::Pat::Ident(ident) = &decl.name {
                    let name = ident.id.sym.to_string();
                    let slot = builder.local_for(&name);
                    let _sym = builder.intern(&name);
                    builder
                        .bindings
                        .insert(name, crate::shadow::Binding::Local(slot));
                }
            }
        }
    }
}

/// Lower a single swc statement into a shadow tree node.
pub fn lower_shadow_stmt<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, crate::shadow::Binding>,
    next_local: &mut u16,
    stmt: &swc_ecma_ast::Stmt,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    match stmt {
        swc_ecma_ast::Stmt::Expr(expr_stmt) => {
            lower_shadow_expr(bump, interner, shapes, bindings, next_local, &expr_stmt.expr)
        }
        swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::Var(var_decl)) => {
            lower_shadow_var_decl(bump, interner, shapes, bindings, next_local, var_decl)
        }
        _ => Err(JsError(format!(
            "unsupported shadow statement: {:?}",
            stmt
        ))),
    }
}

fn lower_shadow_var_decl<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, crate::shadow::Binding>,
    next_local: &mut u16,
    var_decl: &swc_ecma_ast::VarDecl,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    let mut last = None;
    for decl in &var_decl.decls {
        if let swc_ecma_ast::Pat::Ident(ident) = &decl.name {
            let name = ident.id.sym.to_string();
            let slot = var_slot(&name, bindings, next_local);
            if let Some(init) = &decl.init {
                let value =
                    lower_shadow_expr(bump, interner, shapes, bindings, next_local, init)?;
                let node = bump.alloc(crate::shadow::ShadowNode::StoreLocal { index: slot, value });
                last = Some(node);
            }
        }
    }
    Ok(last.unwrap_or_else(|| bump.alloc(crate::shadow::ShadowNode::This)))
}

/// Look up the local slot for a declared variable, allocating a fresh slot if
/// the binding has not been collected.
fn var_slot(
    name: &str,
    bindings: &HashMap<String, crate::shadow::Binding>,
    next_local: &mut u16,
) -> u16 {
    if let Some(crate::shadow::Binding::Local(slot)) = bindings.get(name) {
        *slot
    } else {
        let slot = *next_local;
        *next_local += 1;
        slot
    }
}

/// Lower a single swc expression into a shadow tree node.
pub fn lower_shadow_expr<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, crate::shadow::Binding>,
    next_local: &mut u16,
    expr: &swc_ecma_ast::Expr,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    match expr {
        swc_ecma_ast::Expr::Bin(bin) => lower_binary_expr(bump, interner, shapes, bindings, next_local, bin),
        swc_ecma_ast::Expr::Ident(ident) => lower_ident_expr(bump, interner, bindings, ident),
        swc_ecma_ast::Expr::Lit(lit) => lower_literal_expr(bump, interner, lit),
        swc_ecma_ast::Expr::Object(obj_lit) => lower_object_expr(bump, interner, shapes, obj_lit),
        swc_ecma_ast::Expr::Member(member) => lower_member_expr(bump, interner, shapes, bindings, next_local, member),
        _ => Err(JsError(format!(
            "unsupported shadow expression: {:?}",
            expr
        ))),
    }
}

fn lower_binary_expr<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, crate::shadow::Binding>,
    next_local: &mut u16,
    bin: &swc_ecma_ast::BinExpr,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    let left =
        lower_shadow_expr(bump, interner, shapes, bindings, next_local, &bin.left)?;
    let right =
        lower_shadow_expr(bump, interner, shapes, bindings, next_local, &bin.right)?;
    let node = match bin.op {
        swc_ecma_ast::BinaryOp::Add => crate::shadow::ShadowNode::Add {
            left,
            right,
            state: Cell::new(crate::shadow::AddState::Uninitialized),
            hint: crate::shadow::TypeHint::Any,
        },
        swc_ecma_ast::BinaryOp::Sub => crate::shadow::ShadowNode::Sub { left, right },
        swc_ecma_ast::BinaryOp::Mul => crate::shadow::ShadowNode::Mul { left, right },
        swc_ecma_ast::BinaryOp::Div => crate::shadow::ShadowNode::Div { left, right },
        _ => {
            return Err(JsError(format!(
                "unsupported binary operator: {:?}",
                bin.op
            )))
        }
    };
    Ok(bump.alloc(node))
}

fn lower_ident_expr<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    bindings: &HashMap<String, crate::shadow::Binding>,
    ident: &swc_ecma_ast::Ident,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    let name = ident.sym.to_string();
    if let Some(binding) = bindings.get(&name) {
        Ok(bump.alloc(crate::shadow::ShadowNode::BindingRead(*binding)))
    } else {
        let sym = interner.intern(&name);
        Ok(bump.alloc(crate::shadow::ShadowNode::GlobalRead(sym)))
    }
}

fn lower_literal_expr<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    lit: &swc_ecma_ast::Lit,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    match lit {
        swc_ecma_ast::Lit::Num(num) => {
            let v = num.value;
            if v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64 {
                Ok(bump.alloc(crate::shadow::ShadowNode::LiteralInt(v as i32)))
            } else {
                Ok(bump.alloc(crate::shadow::ShadowNode::LiteralDouble(v)))
            }
        }
        swc_ecma_ast::Lit::Str(s) => {
            let sym = interner.intern(s.value.as_str().unwrap_or(""));
            Ok(bump.alloc(crate::shadow::ShadowNode::LiteralString(sym)))
        }
        swc_ecma_ast::Lit::Bool(b) => Ok(bump.alloc(crate::shadow::ShadowNode::LiteralInt(
            if b.value { 1 } else { 0 },
        ))),
        swc_ecma_ast::Lit::Null(_) => Ok(bump.alloc(crate::shadow::ShadowNode::This)),
        _ => Err(JsError(format!("unsupported literal: {:?}", lit))),
    }
}

fn lower_object_expr<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    obj_lit: &swc_ecma_ast::ObjectLit,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    let mut prop_names = Vec::new();
    let mut prop_values: Vec<(crate::interner::Symbol, &std::boxed::Box<swc_ecma_ast::Expr>)> = Vec::new();
    for prop in &obj_lit.props {
        match prop {
            swc_ecma_ast::PropOrSpread::Prop(prop) => {
                if let swc_ecma_ast::Prop::KeyValue(kv) = prop.as_ref() {
                    let sym = extract_prop_name(interner, &kv.key)?;
                    prop_names.push(sym);
                    prop_values.push((sym, &kv.value));
                } else {
                    return Err(JsError("unsupported object property".into()));
                }
            }
            _ => return Err(JsError("unsupported spread property".into())),
        }
    }
    let shape = shapes.shape_for(&prop_names);
    let mut node: &crate::shadow::ShadowNode = bump.alloc(crate::shadow::ShadowNode::NewObject {
        shape: shape.clone(),
    });
    for (sym, value_expr) in prop_values {
        let value = lower_shadow_expr(bump, interner, shapes, &HashMap::new(), &mut 0, value_expr.as_ref())?;
        let offset = shape.find_offset(sym).ok_or_else(|| JsError("shape missing property".into()))?;
        let (store_offset, is_inline) = compute_store_offset(offset);
        node = bump.alloc(crate::shadow::ShadowNode::StaticPropWrite {
            obj: node, prop: sym, shape_id: shape.id, offset: store_offset, is_inline, value,
        });
    }
    Ok(node)
}

fn extract_prop_name(interner: &mut StringInterner, key: &swc_ecma_ast::PropName) -> Result<crate::interner::Symbol, JsError> {
    let name = match key {
        swc_ecma_ast::PropName::Ident(ident) => ident.sym.to_string(),
        swc_ecma_ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        _ => return Err(JsError("unsupported object key".into())),
    };
    Ok(interner.intern(&name))
}

fn compute_store_offset(offset: usize) -> (u16, bool) {
    let is_inline = offset < crate::shadow::INLINE_SLOTS;
    let store_offset = if is_inline { offset } else { offset - crate::shadow::INLINE_SLOTS };
    (store_offset as u16, is_inline)
}

fn lower_member_expr<'bump>(
    bump: &'bump Bump,
    interner: &mut StringInterner,
    shapes: &ShapeInterner,
    bindings: &HashMap<String, crate::shadow::Binding>,
    next_local: &mut u16,
    member: &swc_ecma_ast::MemberExpr,
) -> Result<&'bump crate::shadow::ShadowNode<'bump>, JsError> {
    let obj =
        lower_shadow_expr(bump, interner, shapes, bindings, next_local, &member.obj)?;
    let prop_name = match &member.prop {
        swc_ecma_ast::MemberProp::Ident(ident) => ident.sym.to_string(),
        _ => return Err(JsError("unsupported member property".into())),
    };
    let sym = interner.intern(&prop_name);
    Ok(bump.alloc(crate::shadow::ShadowNode::PropRead {
        obj,
        prop: sym,
        cache: Cell::new(crate::shadow::PropCache::default()),
    }))
}

/// Convert a nanbox JSValue to a runtime Value.
pub fn jsvalue_to_value(js: crate::nanbox::JSValue, interner: &StringInterner) -> crate::Value {
    if js.is_undefined() {
        crate::Value::Undefined
    } else if js.is_null() {
        crate::Value::Null
    } else if js.is_true() {
        crate::Value::Boolean(true)
    } else if js.is_false() {
        crate::Value::Boolean(false)
    } else if js.is_int32() {
        crate::Value::Number(js.as_int32_unchecked() as f64)
    } else if js.is_double() {
        crate::Value::Number(js.as_double_unchecked())
    } else if js.is_object() {
        crate::Value::ObjectId(js.as_object().expect("nanbox claimed object"))
    } else if js.is_string() {
        let sym = js.as_string().unwrap();
        let s = interner.resolve(sym).unwrap_or("").to_string();
        crate::Value::String(s)
    } else if js.is_symbol() {
        let sym = js.as_symbol().unwrap();
        let s = interner.resolve(sym).unwrap_or("").to_string();
        crate::Value::Symbol(s)
    } else {
        crate::Value::Undefined
    }
}
