//! # Member Expression Emitter
//!
//! Emits Rust member expressions and object literals.

use super::{CodeEmitter, emit_expr, to_snake_case};
use swc_ecma_ast::{Expr, ObjectLit, PropName};

/// Emit a member expression.
/// This handles property access. For method calls, use emit_call which wraps this.
pub fn emit_member(emitter: &mut CodeEmitter, member_expr: &swc_ecma_ast::MemberExpr) {
    // Check if this is a nested member expression (property access vs method call)
    let is_nested = matches!(&*member_expr.obj, swc_ecma_ast::Expr::Member(_));
    
    emit_expr(emitter, &member_expr.obj);
    match &member_expr.prop {
        swc_ecma_ast::MemberProp::Ident(ident) => {
            let prop_name = ident.sym.as_ref();
            // For property access (not method calls), just emit the property name
            // Method calls are handled by emit_call which wraps this
            emit_property_access(emitter, prop_name, is_nested);
        }
        swc_ecma_ast::MemberProp::PrivateName(_) => {
            emitter.push_str(".prop");
        }
        swc_ecma_ast::MemberProp::Computed(comp) => {
            let obj_type = infer_type_from_expr(&member_expr.obj);
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
    }
}

/// Emit a property access (not a method call).
fn emit_property_access(emitter: &mut CodeEmitter, prop_name: &str, is_nested: bool) {
    // For nested property access like props.task.done, just emit the property
    if is_nested {
        emitter.push_str(".");
        emitter.push_str(prop_name);
        return;
    }
    
    // For top-level property access, use emit_method_name to handle built-in methods
    // but avoid adding () for regular properties
    emit_method_name_no_call(emitter, prop_name);
}

/// Infer type from expression (helper).
fn infer_type_from_expr(expr: &Expr) -> String {
    use swc_ecma_ast::{Lit, Callee};
    match expr {
        Expr::Lit(lit) => match lit {
            Lit::Num(_) => "f64".to_string(),
            Lit::Str(_) => "String".to_string(),
            Lit::Bool(_) => "bool".to_string(),
            _ => "()".to_string(),
        },
        Expr::Array(_) => "Vec<()>".to_string(),
        Expr::Object(_) => "()".to_string(),
        Expr::Call(call_expr) => {
            let Callee::Expr(callee) = &call_expr.callee else {
                return "()".to_string();
            };
            if let Expr::Member(member) = &**callee {
                if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
                    let method = prop.sym.as_ref();
                    match method {
                        "filter" | "map" | "concat" | "slice" | "flat" | "flatMap" => {
                            infer_type_from_expr(&member.obj)
                        }
                        "find" | "findIndex" => "Option<()>".to_string(),
                        "some" | "every" | "includes" | "startsWith" | "endsWith" => {
                            "bool".to_string()
                        }
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
        Expr::Member(member_expr) => {
            if let swc_ecma_ast::MemberProp::Ident(prop) = &member_expr.prop {
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
        _ => "()".to_string(),
    }
}

/// Emit a method name with proper Rust mapping.
fn emit_method_name(emitter: &mut CodeEmitter, prop_name: &str) {
    // Check if this is a PascalCase identifier (likely an enum variant)
    // Enum variants are accessed as EnumName::VariantName in Rust
    let is_pascal_case = prop_name.chars().next().is_some_and(|c| c.is_uppercase());
    
    if is_pascal_case {
        // This is likely an enum variant access like KeyCode.Up
        // In Rust, this should be KeyCode::Up
        emitter.push_str("::");
        emitter.push_str(prop_name);
    } else {
        emit_standard_method_name(emitter, prop_name);
    }
}

/// Emit method name without adding () - for property access context.
fn emit_method_name_no_call(emitter: &mut CodeEmitter, prop_name: &str) {
    // Check if PascalCase (enum variant)
    let is_pascal_case = prop_name.chars().next().is_some_and(|c| c.is_uppercase());
    
    if is_pascal_case {
        emitter.push_str("::");
        emitter.push_str(prop_name);
        return;
    }
    
    // For property access (no method call), just emit the property with proper Rust mapping
    match prop_name {
        "length" => emitter.push_str(".len()"),
        "toString" | "valueOf" => {}
        "toLowerCase" => emitter.push_str(".to_lowercase()"),
        "toUpperCase" => emitter.push_str(".to_uppercase()"),
        "trim" => emitter.push_str(".trim()"),
        "trimStart" | "trimLeft" => emitter.push_str(".trim_start()"),
        "trimEnd" | "trimRight" => emitter.push_str(".trim_end()"),
        "id" | "title" | "done" | "tasks" | "selected" | "shouldExit" 
        | "showCompleted" | "show_completed" | "active"
        | "task" | "props" | "on_click" | "on_select" | "onChange" => {
            emitter.push_str(".");
            emitter.push_str(&to_snake_case(prop_name));
        }
        // Methods without args - property-like
        "pop" => emitter.push_str(".pop()"),
        // For other properties, just emit as property access
        _ => {
            emitter.push_str(".");
            emitter.push_str(prop_name);
        }
    }
}

/// Emit standard method names (with method call syntax).
fn emit_standard_method_name(emitter: &mut CodeEmitter, prop_name: &str) {
    match prop_name {
        "length" => emitter.push_str(".len()"),
        "toString" => {}
        "valueOf" => {}
        "toLowerCase" => emitter.push_str(".to_lowercase()"),
        "toUpperCase" => emitter.push_str(".to_uppercase()"),
        "trim" => emitter.push_str(".trim()"),
        "trimStart" | "trimLeft" => emitter.push_str(".trim_start()"),
        "trimEnd" | "trimRight" => emitter.push_str(".trim_end()"),
        "includes" => emitter.push_str(".contains("),
        "startsWith" => emitter.push_str(".starts_with("),
        "endsWith" => emitter.push_str(".ends_with("),
        "indexOf" => emitter.push_str(".find("),
        "lastIndexOf" => emitter.push_str(".rfind("),
        "charAt" => emitter.push_str(".chars().next()"),
        "charCodeAt" => emitter.push_str(".chars().next().map(|c| c as u32)"),
        "split" => emitter.push_str(".split("),
        "toFixed" => emitter.push_str(".trunc()"),
        "push" => emitter.push_str(".push("),
        "pop" => emitter.push_str(".pop()"),
        "shift" => emitter.push_str(".remove(0)"),
        "unshift" => emitter.push_str(".insert(0, "),
        "filter" => emitter.push_str(".iter().filter("),
        "map" => emitter.push_str(".iter().map("),
        "reduce" => emitter.push_str(".iter().fold("),
        "forEach" => emitter.push_str(".iter().for_each("),
        "some" => emitter.push_str(".iter().any("),
        "every" => emitter.push_str(".iter().all("),
        "find" => emitter.push_str(".iter().find("),
        "findIndex" => emitter.push_str(".iter().position("),
        "concat" => emitter.push_str(".extend("),
        "join" => emitter.push_str(".join("),
        "reverse" => emitter.push_str(".reverse()"),
        "sort" => emitter.push_str(".sort_by(|a, b| a.cmp(b))"),
        "slice" => emitter.push_str(".iter()"),
        "fill" => emitter.push_str(".fill("),
        "copyWithin" => emitter.push_str(".copy_within("),
        "entries" => emitter.push_str(".iter().enumerate()"),
        "keys" => emitter.push_str(".iter().enumerate()"),
        "values" => emitter.push_str(".iter()"),
        "flat" => emitter.push_str(".iter().flatten().collect::<Vec<_>>()"),
        "flatMap" => emitter.push_str(".iter().flat_map(|x| x).collect::<Vec<_>>()"),
        "now" => emitter.push_str(".now()"),
        "getTime" => emitter.push_str(
            ".duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64",
        ),
        "getFullYear" => emitter.push_str(".format(\"%Y\").to_string()"),
        "getMonth" => emitter.push_str(".format(\"%m\").parse::<u32>().ok()"),
        "getDate" => emitter.push_str(".format(\"%d\").parse::<u32>().ok()"),
        "id" | "title" | "done" | "tasks" | "selected" | "shouldExit" => {
            emitter.push_str(".");
            emitter.push_str(&to_snake_case(prop_name));
        }
        _ => {
            emitter.push_str(".");
            emitter.push_str(&to_snake_case(prop_name));
            emitter.push_str("(");
        }
    }
}

/// Emit an object literal, with struct name context if available.
pub fn emit_object(emitter: &mut CodeEmitter, obj: &ObjectLit) {
    let has_struct = emitter.object_struct_name().is_some();
    if has_struct {
        let name = emitter.object_struct_name().unwrap().clone();
        emitter.push_str(&name);
        emitter.push_str(" { ");
    } else {
        emitter.push_str("{ ");
    }

    let mut first = true;
    for prop in &obj.props {
        match prop {
            swc_ecma_ast::PropOrSpread::Prop(prop) => {
                if !first {
                    emitter.push_str(", ");
                }
                first = false;
                match &**prop {
                    swc_ecma_ast::Prop::KeyValue(kv) => {
                        emit_prop_key(emitter, &kv.key);
                        emitter.push_str(": ");
                        emit_expr(emitter, &kv.value);
                    }
                    swc_ecma_ast::Prop::Shorthand(ident) => {
                        let name = to_snake_case(ident.sym.as_ref());
                        emitter.push_str(&name);
                        emitter.push_str(": ");
                        emitter.push_str(&name);
                    }
                    swc_ecma_ast::Prop::Assign(kv) => {
                        emitter.push_str(&to_snake_case(kv.key.sym.as_ref()));
                        emitter.push_str(": ");
                        emit_expr(emitter, &kv.value);
                    }
                    swc_ecma_ast::Prop::Getter(_) | swc_ecma_ast::Prop::Setter(_) => {
                        emitter.push_str("/* getter/setter */ ()");
                    }
                    swc_ecma_ast::Prop::Method(_) => {
                        emitter.push_str("/* method */ ()");
                    }
                }
            }
            swc_ecma_ast::PropOrSpread::Spread(spread) => {
                if !first {
                    emitter.push_str(", ");
                }
                first = false;
                emitter.push_str("..");
                emit_expr(emitter, &spread.expr);
            }
        }
    }

    emitter.push_str(" }");
}

/// Emit object property key.
fn emit_prop_key(emitter: &mut CodeEmitter, key: &PropName) {
    match key {
        PropName::Ident(ident) => {
            emitter.push_str(&to_snake_case(ident.sym.as_ref()));
        }
        PropName::Str(s) => {
            emitter.push_str(&to_snake_case(&format!("{:?}", s.value)));
        }
        PropName::Num(n) => {
            emitter.push_str(&n.value.to_string());
        }
        _ => emitter.push_str("unknown"),
    }
}
