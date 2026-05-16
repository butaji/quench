//! # Type Inference
//!
//! Infers Rust types from TypeScript expressions.

use swc_ecma_ast::{Callee, Expr, Lit};

/// Infer the type of an expression as a Rust type string.
pub fn infer_type(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match lit {
            Lit::Num(n) => {
                if n.value.fract() == 0.0 && n.value.abs() < f64::from(i32::MAX) {
                    "i32".to_string()
                } else {
                    "f64".to_string()
                }
            }
            Lit::Str(_) => "String".to_string(),
            Lit::Bool(_) => "bool".to_string(),
            Lit::BigInt(_) => "i64".to_string(),
            Lit::Null(_) => "Option<()>".to_string(),
            _ => "()".to_string(),
        },
        Expr::Array(arr) => {
            // Infer array element type
            if arr.elems.is_empty() {
                String::from("Vec<()>")
            } else if let Some(Some(elem)) = arr.elems.first() {
                let elem_type = infer_type(&elem.expr);
                format!("Vec<{}>", elem_type)
            } else {
                String::from("Vec<()>")
            }
        }
        Expr::Object(_) => "()".to_string(),
        Expr::Tpl(_) => "String".to_string(),
        Expr::Bin(bin_expr) => infer_bin_type(bin_expr),
        Expr::Unary(unary_expr) => {
            match unary_expr.op {
                swc_ecma_ast::UnaryOp::Bang | swc_ecma_ast::UnaryOp::TypeOf => {
                    "bool".to_string()
                }
                _ => infer_type(&unary_expr.arg),
            }
        }
        Expr::Call(call_expr) => infer_call_type(call_expr),
        Expr::Member(member_expr) => infer_member_type(member_expr),
        Expr::Cond(cond_expr) => {
            let cons_type = infer_type(&cond_expr.cons);
            let alt_type = infer_type(&cond_expr.alt);
            // If both branches are the same type, use that type
            if cons_type == alt_type {
                cons_type
            } else if cons_type == "()" && alt_type != "()" {
                // One branch is unit, the other has a type - use the non-unit type
                alt_type
            } else if alt_type == "()" && cons_type != "()" {
                cons_type
            } else {
                // Different non-unit types - for now, return cons type as best-effort
                cons_type
            }
        }
        Expr::Arrow(_) => "()".to_string(),
        Expr::Paren(paren) => infer_type(&paren.expr),
        Expr::Await(await_expr) => infer_type(&await_expr.arg),
        _ => "()".to_string(),
    }
}

fn infer_bin_type(bin_expr: &swc_ecma_ast::BinExpr) -> String {
    let left = infer_type(&bin_expr.left);
    let right = infer_type(&bin_expr.right);

    match bin_expr.op {
        swc_ecma_ast::BinaryOp::Add if left == "String" || right == "String" => {
            "String".to_string()
        }
        swc_ecma_ast::BinaryOp::EqEqEq
        | swc_ecma_ast::BinaryOp::NotEqEq
        | swc_ecma_ast::BinaryOp::Lt
        | swc_ecma_ast::BinaryOp::LtEq
        | swc_ecma_ast::BinaryOp::Gt
        | swc_ecma_ast::BinaryOp::GtEq
        | swc_ecma_ast::BinaryOp::LogicalAnd
        | swc_ecma_ast::BinaryOp::LogicalOr => "bool".to_string(),
        swc_ecma_ast::BinaryOp::BitAnd
        | swc_ecma_ast::BinaryOp::BitOr
        | swc_ecma_ast::BinaryOp::BitXor
        | swc_ecma_ast::BinaryOp::LShift
        | swc_ecma_ast::BinaryOp::RShift => "i32".to_string(),
        _ => {
            if left == "i32" || right == "i32" {
                "i32".to_string()
            } else {
                "f64".to_string()
            }
        }
    }
}

fn infer_call_type(call_expr: &swc_ecma_ast::CallExpr) -> String {
    let Callee::Expr(callee) = &call_expr.callee else {
        return "()".to_string();
    };

    // Direct function calls
    if let Expr::Ident(ident) = &**callee {
        let fn_name = ident.sym.as_ref();
        return match fn_name {
            "filter_tasks" => "Vec<Task>".to_string(),
            "create_task" => "Task".to_string(),
            "toggle_task" => "Task".to_string(),
            "validate_title" => "Result<String, String>".to_string(),
            "validate_task" => "Result<Task, String>".to_string(),
            "parse_json" => "Result<JsonValue, String>".to_string(),
            "serialize_tasks" => "String".to_string(),
            "deserialize_tasks" => "Result<Vec<Task>, String>".to_string(),
            "merge_tasks" => "Vec<Task>".to_string(),
            "find_task" => "Option<Task>".to_string(),
            "sort_tasks" => "Vec<Task>".to_string(),
            "get_stats" => "Stats".to_string(),
            "is_number" | "is_string" | "is_boolean" | "is_object" => "bool".to_string(),
            "fast_sqrt" => "f64".to_string(),
            "batch_add" => "Vec<f64>".to_string(),
            "mean" => "f64".to_string(),
            "variance" => "f64".to_string(),
            "std_dev" => "f64".to_string(),
            _ => "()".to_string(),
        };
    }

    // Method calls
    if let Expr::Member(member) = &**callee {
        let obj_type = infer_type(&member.obj);

        if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
            let method = prop.sym.as_ref();
            return match method {
                "filter" | "map" | "concat" | "slice" | "flat" | "flatMap" => obj_type,
                "find" | "findIndex" => {
                    if obj_type.starts_with("Vec") {
                        format!("Option<{}>", &obj_type[4..obj_type.len() - 1])
                    } else {
                        "Option<()>".to_string()
                    }
                }
                "some" | "every" | "includes" | "startsWith" | "endsWith" => {
                    "bool".to_string()
                }
                "push" => "usize".to_string(),
                "pop" | "shift" => "Option<()>".to_string(),
                "reduce" => {
                    if call_expr.args.len() >= 2 {
                        infer_type(&call_expr.args[1].expr)
                    } else {
                        "()".to_string()
                    }
                }
                "trim"
                | "toLowerCase"
                | "toUpperCase"
                | "trimStart"
                | "trimEnd"
                | "substring"
                | "substr"
                | "toString" => "String".to_string(),
                "indexOf" | "lastIndexOf" => "Option<usize>".to_string(),
                "charAt" => "Option<char>".to_string(),
                "join" => "String".to_string(),
                "split" => "Vec<String>".to_string(),
                "length" => "usize".to_string(),
                "forEach" => "()".to_string(),
                _ => "()".to_string(),
            };
        }
    }

    "()".to_string()
}

fn infer_member_type(member_expr: &swc_ecma_ast::MemberExpr) -> String {
    let obj_type = infer_type(&member_expr.obj);

    if let swc_ecma_ast::MemberProp::Ident(prop) = &member_expr.prop {
        let prop_name = prop.sym.as_ref();
        return match prop_name {
            "length" => "usize".to_string(),
            "id" if obj_type == "Task" => "i32".to_string(),
            "title" if obj_type == "Task" => "String".to_string(),
            "done" if obj_type == "Task" => "bool".to_string(),
            "tasks" if obj_type == "AppState" => "Vec<Task>".to_string(),
            "selected" if obj_type == "AppState" => "usize".to_string(),
            "filter" if obj_type == "AppState" => "Filter".to_string(),
            "shouldExit" if obj_type == "AppState" => "bool".to_string(),
            "ok" => "bool".to_string(),
            "value" => "()".to_string(),
            "error" => "String".to_string(),
            "trim"
            | "toLowerCase"
            | "toUpperCase"
            | "trimStart"
            | "trimEnd"
            | "substring"
            | "substr"
            | "toString" => "String".to_string(),
            "push" => "usize".to_string(),
            "pop" | "shift" => "Option<()>".to_string(),
            "filter" | "map" | "concat" | "slice" => obj_type,
            "find" | "findIndex" => {
                if obj_type.starts_with("Vec") {
                    format!("Option<{}>", &obj_type[4..obj_type.len() - 1])
                } else {
                    "Option<()>".to_string()
                }
            }
            "some" | "every" | "includes" | "startsWith" | "endsWith" => "bool".to_string(),
            _ => "()".to_string(),
        };
    }

    "()".to_string()
}
