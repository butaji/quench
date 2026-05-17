//! # Member Expression Emitter
//!
//! Emits Rust member expressions and object literals.

use super::utils::{escape_rust_keyword, infer_struct_from_object, to_snake_case};
use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::{Expr, MemberProp, ObjectLit, Prop, PropName, PropOrSpread};

/// Emit a member expression.
pub fn emit_member(emitter: &mut CodeEmitter, member_expr: &swc_ecma_ast::MemberExpr) {
    let is_nested = matches!(&*member_expr.obj, Expr::Member(_));
    emit_expr(emitter, &member_expr.obj);

    match &member_expr.prop {
        MemberProp::Ident(ident) => {
            emit_property_access(emitter, ident.sym.as_ref(), is_nested);
        }
        MemberProp::PrivateName(_) => {
            emitter.push_str(".prop");
        }
        MemberProp::Computed(comp) => {
            emit_computed_property(emitter, &member_expr.obj, comp);
        }
    }
}

fn emit_property_access(emitter: &mut CodeEmitter, prop_name: &str, is_nested: bool) {
    if is_nested {
        emit_nested_property(emitter, prop_name);
    } else {
        emit_top_level_property(emitter, prop_name);
    }
}

fn emit_nested_property(emitter: &mut CodeEmitter, prop_name: &str) {
    if prop_name == "length" {
        emitter.push_str(".len()");
    } else {
        emitter.push_str(".");
        emitter.push_str(prop_name);
    }
}

fn emit_top_level_property(emitter: &mut CodeEmitter, prop_name: &str) {
    let is_pascal_case = prop_name.chars().next().is_some_and(|c| c.is_uppercase());

    if is_pascal_case {
        emitter.push_str("::");
        emitter.push_str(prop_name);
        return;
    }

    match prop_name {
        "length" => emitter.push_str(".len()"),
        "toString" | "valueOf" => {}
        "toLowerCase" => emitter.push_str(".to_lowercase()"),
        "toUpperCase" => emitter.push_str(".to_uppercase()"),
        "trim" => emitter.push_str(".trim()"),
        "trimStart" | "trimLeft" => emitter.push_str(".trim_start()"),
        "trimEnd" | "trimRight" => emitter.push_str(".trim_end()"),
        "pop" => emitter.push_str(".pop()"),
        _ => {
            emitter.push_str(".");
            emitter.push_str(prop_name);
        }
    }
}

fn emit_computed_property(
    emitter: &mut CodeEmitter,
    obj: &Expr,
    comp: &swc_ecma_ast::ComputedPropName,
) {
    let obj_type = infer_type_from_expr(obj);
    if obj_type.starts_with("Vec") {
        emitter.push_str("[");
        emit_expr(emitter, &comp.expr);
        emitter.push_str(" as usize]");
    } else {
        emitter.push_str(".get(");
        emit_expr(emitter, &comp.expr);
        emitter.push_str(")");
    }
}

fn infer_type_from_expr(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match lit {
            swc_ecma_ast::Lit::Num(_) => "f64".to_string(),
            swc_ecma_ast::Lit::Str(_) => "String".to_string(),
            swc_ecma_ast::Lit::Bool(_) => "bool".to_string(),
            _ => "()".to_string(),
        },
        Expr::Array(_) => "Vec<()>".to_string(),
        Expr::Object(_) => "()".to_string(),
        Expr::Call(call_expr) => infer_type_from_call(call_expr),
        Expr::Member(member_expr) => infer_type_from_member(member_expr),
        _ => "()".to_string(),
    }
}

fn infer_type_from_call(call_expr: &swc_ecma_ast::CallExpr) -> String {
    let swc_ecma_ast::Callee::Expr(callee) = &call_expr.callee else {
        return "()".to_string();
    };

    if let Expr::Member(member) = &**callee {
        if let MemberProp::Ident(prop) = &member.prop {
            let method = prop.sym.as_ref();
            match method {
                "filter" | "map" | "concat" | "slice" | "flat" | "flatMap" => {
                    infer_type_from_expr(&member.obj)
                }
                "find" | "findIndex" => "Option<()>".to_string(),
                "some" | "every" | "includes" | "startsWith" | "endsWith" => "bool".to_string(),
                "push" => "usize".to_string(),
                "pop" | "shift" => "Option<()>".to_string(),
                "length" => "usize".to_string(),
                _ => "()".to_string(),
            }
        } else {
            "()".to_string()
        }
    } else {
        "()".to_string()
    }
}

fn infer_type_from_member(member_expr: &swc_ecma_ast::MemberExpr) -> String {
    if let MemberProp::Ident(prop) = &member_expr.prop {
        match prop.sym.as_ref() {
            "length" => "usize".to_string(),
            "id" => "i32".to_string(),
            "title" | "error" => "String".to_string(),
            "done" | "ok" => "bool".to_string(),
            _ => "()".to_string(),
        }
    } else {
        "()".to_string()
    }
}

/// Emit an object literal, with struct name context if available.
pub fn emit_object(emitter: &mut CodeEmitter, obj: &ObjectLit) {
    let struct_name = resolve_struct_name(emitter, obj);
    let spread_source = find_spread_source(obj);

    match struct_name {
        StructNameKind::Explicit(name) => {
            emit_struct_literal(emitter, &name, obj, spread_source.as_deref());
        }
        StructNameKind::Inferred(name) => {
            emit_struct_literal(emitter, &name, obj, spread_source.as_deref());
        }
        StructNameKind::ResultPattern => {
            emit_result_pattern_object(emitter, obj);
        }
        StructNameKind::Anonymous => {
            emit_anonymous_object(emitter, obj, spread_source.as_deref());
        }
    }
}

enum StructNameKind {
    Explicit(String),
    Inferred(String),
    ResultPattern,
    Anonymous,
}

fn resolve_struct_name(emitter: &CodeEmitter, obj: &ObjectLit) -> StructNameKind {
    if let Some(name) = emitter.object_struct_name().cloned() {
        return StructNameKind::Explicit(name);
    }

    if let Some(name) = infer_struct_from_object(obj) {
        return StructNameKind::Inferred(name);
    }

    if let Some(name) = emitter.expected_return().cloned() {
        return StructNameKind::Inferred(name);
    }

    if is_result_pattern_object(obj) {
        return StructNameKind::ResultPattern;
    }

    StructNameKind::Anonymous
}

fn emit_struct_literal(
    emitter: &mut CodeEmitter,
    name: &str,
    obj: &ObjectLit,
    spread: Option<&Expr>,
) {
    emitter.push_str(name);
    emitter.push_str(" { ");
    emit_object_props(emitter, obj);
    if let Some(source) = spread {
        emitter.push_str(", ..");
        emit_expr(emitter, source);
    }
    emitter.push_str(" }");
}

fn emit_result_pattern_object(emitter: &mut CodeEmitter, obj: &ObjectLit) {
    if let Some(expr) = extract_result_value(obj) {
        emitter.push_str("Ok(");
        emit_expr(emitter, expr);
        emitter.push_str(")");
    } else if let Some(expr) = extract_result_error(obj) {
        emitter.push_str("Err(");
        emit_expr(emitter, expr);
        emitter.push_str(")");
    } else {
        emitter.push_str("{ ");
        emit_object_props(emitter, obj);
        emitter.push_str(" }");
    }
}

fn emit_anonymous_object(emitter: &mut CodeEmitter, obj: &ObjectLit, spread: Option<&Expr>) {
    if spread.is_some() {
        emitter.push_str("{ /* struct update without type context */ }");
    } else {
        emitter.push_str("{ ");
        emit_object_props(emitter, obj);
        emitter.push_str(" }");
    }
}

fn is_result_pattern_object(obj: &ObjectLit) -> bool {
    obj.props.iter().any(|p| {
        if let PropOrSpread::Prop(prop) = p {
            if let Prop::KeyValue(kv) = &**prop {
                if let PropName::Ident(ident) = &kv.key {
                    let name = ident.sym.as_ref();
                    return name == "ok" || name == "value" || name == "error";
                }
            }
        }
        false
    })
}

fn extract_result_value(obj: &ObjectLit) -> Option<&Expr> {
    obj.props.iter().find_map(|p| {
        if let PropOrSpread::Prop(prop) = p {
            if let Prop::KeyValue(kv) = &**prop {
                if let PropName::Ident(ident) = &kv.key {
                    if ident.sym.as_ref() == "value" {
                        return Some(&*kv.value);
                    }
                }
            }
        }
        None
    })
}

fn extract_result_error(obj: &ObjectLit) -> Option<&Expr> {
    obj.props.iter().find_map(|p| {
        if let PropOrSpread::Prop(prop) = p {
            if let Prop::KeyValue(kv) = &**prop {
                if let PropName::Ident(ident) = &kv.key {
                    if ident.sym.as_ref() == "error" {
                        return Some(&*kv.value);
                    }
                }
            }
        }
        None
    })
}

fn find_spread_source(obj: &ObjectLit) -> Option<Box<Expr>> {
    obj.props.iter().find_map(|p| {
        if let PropOrSpread::Spread(spread) = p {
            Some(spread.expr.clone())
        } else {
            None
        }
    })
}

fn emit_object_props(emitter: &mut CodeEmitter, obj: &ObjectLit) {
    let mut first = true;

    for prop in &obj.props {
        if let PropOrSpread::Prop(prop) = prop {
            if !first {
                emitter.push_str(", ");
            }
            first = false;
            emit_single_prop(emitter, prop);
        }
    }
}

fn emit_single_prop(emitter: &mut CodeEmitter, prop: &Prop) {
    match prop {
        Prop::KeyValue(kv) => {
            emit_prop_key(emitter, &kv.key);
            emitter.push_str(": ");
            emit_expr(emitter, &kv.value);
        }
        Prop::Shorthand(ident) => {
            let name = escape_rust_keyword(ident.sym.as_ref());
            emitter.push_str(&name);
            emitter.push_str(": ");
            emitter.push_str(&name);
        }
        Prop::Assign(kv) => {
            let name = escape_rust_keyword(kv.key.sym.as_ref());
            emitter.push_str(&name);
            emitter.push_str(": ");
            emit_expr(emitter, &kv.value);
        }
        Prop::Getter(_) | Prop::Setter(_) => {
            emitter.push_str("/* getter/setter */ ()");
        }
        Prop::Method(_) => {
            emitter.push_str("/* method */ ()");
        }
    }
}

fn emit_prop_key(emitter: &mut CodeEmitter, key: &PropName) {
    match key {
        PropName::Ident(ident) => {
            emitter.push_str(&escape_rust_keyword(ident.sym.as_ref()));
        }
        PropName::Str(s) => {
            let name = to_snake_case(&format!("{:?}", s.value));
            emitter.push_str(&escape_rust_keyword(&name));
        }
        PropName::Num(n) => {
            emitter.push_str(&n.value.to_string());
        }
        PropName::Computed(_) => {
            emitter.push_str("/* computed */ ()");
        }
        PropName::BigInt(_) => emitter.push_str("unknown"),
    }
}
