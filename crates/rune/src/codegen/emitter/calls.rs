//! # Call Expression Emitter
//!
//! Emits Rust function calls from TypeScript.

use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::{Callee, Expr};

/// Emit a function call with built-in handling.
pub fn emit_call(emitter: &mut CodeEmitter, call_expr: &swc_ecma_ast::CallExpr) {
    let Callee::Expr(callee) = &call_expr.callee else {
        emitter.push_str("/* unknown callee */ ()");
        return;
    };

    // Handle member expressions (obj.method())
    if let Expr::Member(member) = &**callee {
        // Check for built-in objects
        if let Expr::Ident(ident) = &*member.obj {
            let obj_name = ident.sym.as_ref();

            // Date.now()
            if obj_name == "Date"
                && matches!(
                    &member.prop,
                    swc_ecma_ast::MemberProp::Ident(p) if p.sym.as_ref() == "now"
                )
            {
                emitter.push_str(
                    "std::time::SystemTime::now()\
                    .duration_since(std::time::UNIX_EPOCH)\
                    .unwrap()\
                    .as_millis() as i64",
                );
                return;
            }

            // JSON methods
            if obj_name == "JSON" {
                if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
                    match prop.sym.as_ref() {
                        "stringify" => {
                            if let Some(arg) = call_expr.args.first() {
                                emitter.push_str("serde_json::to_string(&");
                                emit_expr(emitter, &arg.expr);
                                emitter.push_str(").unwrap_or_default()");
                            } else {
                                emitter.push_str("String::new()");
                            }
                            return;
                        }
                        "parse" => {
                            if let Some(arg) = call_expr.args.first() {
                                emitter.push_str(
                                    "serde_json::from_str::<serde_json::Value>(&",
                                );
                                emit_expr(emitter, &arg.expr);
                                emitter.push_str(").ok()");
                            } else {
                                emitter.push_str("None");
                            }
                            return;
                        }
                        _ => {}
                    }
                }
            }

            // Math methods
            if obj_name == "Math" {
                if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
                    let fn_name = match prop.sym.as_ref() {
                        "floor" => "floor",
                        "ceil" => "ceil",
                        "round" => "round",
                        "abs" => "abs",
                        "sqrt" => "sqrt",
                        "pow" => "powf",
                        "max" => "max",
                        "min" => "min",
                        "random" => "random",
                        m => m,
                    };
                    emitter.push_str(&format!("{fn_name}("));
                    for (i, arg) in call_expr.args.iter().enumerate() {
                        if i > 0 {
                            emitter.push_str(", ");
                        }
                        emit_expr(emitter, &arg.expr);
                    }
                    emitter.push_str(")");
                    return;
                }
            }
        }

        // Method call on any object
        if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
            let method = prop.sym.as_ref();

            // Array methods
            if method == "splice" {
                emit_expr(emitter, &member.obj);
                emitter.push_str(".splice(");
                if let Some(start_arg) = call_expr.args.first() {
                    emit_expr(emitter, &start_arg.expr);
                    emitter.push_str("..");
                    emit_expr(emitter, &start_arg.expr);
                    emitter.push_str(" + ");
                    if call_expr.args.len() >= 2 {
                        emit_expr(emitter, &call_expr.args[1].expr);
                    } else {
                        emitter.push_str("1");
                    }
                }
                emitter.push_str(", vec![])");
                return;
            }

            // Array index access with get()
            if method == "get" {
                emit_expr(emitter, &member.obj);
                emitter.push_str(".get(");
                if let Some(arg) = call_expr.args.first() {
                    emit_expr(emitter, &arg.expr);
                    emitter.push_str(" as usize");
                }
                emitter.push_str(")");
                return;
            }

            // Emit array iterator methods with .iter() prefix
            if matches!(method, "filter" | "map" | "reduce" | "forEach" | "some" | "every" | "find" | "findIndex" | "concat" | "join" | "reverse" | "sort" | "slice") {
                emit_expr(emitter, &member.obj);
                emitter.push_str(".iter().");
                // Map JavaScript method names to Rust equivalents
                let rust_method = match method {
                    "forEach" => "for_each",
                    "findIndex" => "position",
                    "some" => "any",
                    "every" => "all",
                    _ => method,
                };
                emitter.push_str(rust_method);
                emitter.push_str("(");
                for (i, arg) in call_expr.args.iter().enumerate() {
                    if i > 0 {
                        emitter.push_str(", ");
                    }
                    if arg.spread.is_some() {
                        emitter.push_str("/* spread */");
                    }
                    emit_expr(emitter, &arg.expr);
                }
                emitter.push_str(")");
                return;
            }

            // String methods
            match method {
                "localeCompare" => {
                    emitter.push_str("(");
                    emit_expr(emitter, &member.obj);
                    emitter.push_str(".cmp(");
                    if let Some(arg) = call_expr.args.first() {
                        emit_expr(emitter, &arg.expr);
                    }
                    emitter.push_str(") as i32)");
                    return;
                }
                "includes" => {
                    emitter.push_str("(");
                    emit_expr(emitter, &member.obj);
                    emitter.push_str(".contains(");
                    if let Some(arg) = call_expr.args.first() {
                        emit_expr(emitter, &arg.expr);
                    }
                    emitter.push_str("))");
                    return;
                }
                "indexOf" => {
                    emitter.push_str("(");
                    emit_expr(emitter, &member.obj);
                    emitter.push_str(".find(");
                    if let Some(arg) = call_expr.args.first() {
                        emit_expr(emitter, &arg.expr);
                    }
                    emitter.push_str(").is_some() as i32)");
                    return;
                }
                _ => {}
            }

            // Generic method call
            emit_expr(emitter, &member.obj);
            emitter.push_str(&format!(".{method}("));
            for (i, arg) in call_expr.args.iter().enumerate() {
                if i > 0 {
                    emitter.push_str(", ");
                }
                if arg.spread.is_some() {
                    emitter.push_str("/* spread */");
                }
                emit_expr(emitter, &arg.expr);
            }
            emitter.push_str(")");
            return;
        }
    }

    // Direct function call
    if let Expr::Ident(ident) = &**callee {
        let fn_name = super::to_snake_case(ident.sym.as_ref());
        emitter.push_str(&fn_name);
        emitter.push_str("(");
        for (i, arg) in call_expr.args.iter().enumerate() {
            if i > 0 {
                emitter.push_str(", ");
            }
            if arg.spread.is_some() {
                emitter.push_str("/* spread */");
            }
            emit_expr(emitter, &arg.expr);
        }
        emitter.push_str(")");
        return;
    }

    // Generic call expression fallback
    emit_expr(emitter, callee);
    emitter.push_str("(");
    for (i, arg) in call_expr.args.iter().enumerate() {
        if i > 0 {
            emitter.push_str(", ");
        }
        emit_expr(emitter, &arg.expr);
    }
    emitter.push_str(")");
}
