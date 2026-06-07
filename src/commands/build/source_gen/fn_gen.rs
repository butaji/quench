use crate::transpile::hir::QuoteCodegen;

pub(crate) fn generate_function_string(
    codegen: &QuoteCodegen,
    func: &crate::transpile::hir::FunctionDecl,
) -> String {
    let fn_name = &func.name;
    let async_kw = if func.is_async { "async " } else { "" };
    let params_str = gen_params_string(func);
    let ret_type_str = gen_ret_type_string(func);
    let body_str = generate_body_string(codegen, &func.body, &ret_type_str);
    let header = make_fn_header(async_kw, fn_name, &params_str, &ret_type_str);
    make_fn_string(&header, &body_str)
}

pub(crate) fn make_fn_header(async_kw: &str, name: &str, params: &str, ret: &str) -> String {
    let mut s = String::from("pub ");
    s.push_str(async_kw);
    s.push_str("fn ");
    s.push_str(name);
    s.push('(');
    s.push_str(params);
    s.push_str(") -> ");
    s.push_str(ret);
    s.push_str(" {");
    s
}

pub(crate) fn make_fn_string(header: &str, body: &str) -> String {
    header.to_string() + "\n" + body + "}\n"
}

pub(crate) fn gen_params_string(func: &crate::transpile::hir::FunctionDecl) -> String {
    let params: Vec<String> = func.params.iter().map(gen_param_string).collect();
    params.join(", ")
}

pub(crate) fn gen_param_string(p: &crate::transpile::hir::Param) -> String {
    let name = &p.name;
    let ty_str = p.type_.as_ref()
        .map(|t| type_to_rust_string(t))
        .unwrap_or_else(|| "String".to_string());
    format!("{}: {}", name, ty_str)
}

pub(crate) fn gen_ret_type_string(func: &crate::transpile::hir::FunctionDecl) -> String {
    func.return_type.as_ref()
        .map(|t| type_to_rust_string(t))
        .unwrap_or_else(|| {
            infer_return_type_from_body(&func.body).unwrap_or_else(|| "()".to_string())
        })
}

/// Infer return type from return statements in the body
pub(crate) fn infer_return_type_from_body(body: &Option<crate::transpile::hir::Block>) -> Option<String> {
    use crate::transpile::hir::Stmt;

    let body = match body {
        Some(b) => b,
        None => return None,
    };

    // Look for return statements with expressions
    for stmt in &body.0 {
        if let Stmt::Return { arg: Some(expr) } = stmt {
            // Infer type from expression
            return Some(infer_type_from_expr(expr));
        }
    }
    None
}

/// Infer Rust type from expression
pub(crate) fn infer_type_from_expr(expr: &crate::transpile::hir::Expr) -> String {
    use crate::transpile::hir::Expr as E;
    match expr {
        E::String(_) => "String".to_string(),
        E::Number(_) => "f64".to_string(),
        E::Boolean(_) => "bool".to_string(),
        E::Null | E::Undefined => "Value".to_string(),
        E::Bin {
            op,
            left: _,
            right: _,
        } => {
            // For Add with string operands, result is String
            use crate::transpile::hir::BinaryOp;
            if matches!(op, BinaryOp::Add) {
                "String".to_string()
            } else {
                "Value".to_string()
            }
        }
        E::Call { .. } => "Value".to_string(), // TODO: infer from function return type
        _ => "Value".to_string(),
    }
}

/// Convert HIR Type to Rust string representation
pub(crate) fn type_to_rust_string(ty: &crate::transpile::hir::Type) -> String {
    use crate::transpile::hir::Type as T;
    match ty {
        T::String=>"String".to_string(),
        T::Number=>"f64".to_string(),
        T::Boolean=>"bool".to_string(),
        T::Void | T::Never=>"()".to_string(),
        T::Undefined | T::Null | T::Unknown | T::Any=>"Value".to_string(),
        T::BigInt=>"i64".to_string(),
        T::Array { elem }=>format!("Vec<{}>", type_to_rust_string(elem)),
        T::Ref { name, generics }=>ref_type_string(name, generics),
        _=>"Value".to_string(),
    }
}

pub(crate) fn ref_type_string(name: &str, generics: &[crate::transpile::hir::Type]) -> String {
    if generics.is_empty() {
        name.to_string()
    } else {
        let inner = generics
            .iter()
            .map(type_to_rust_string)
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}<{}>", name, inner)
    }
}

/// Generate function body string from HIR Block
pub(crate) fn generate_body_string(
    codegen: &QuoteCodegen,
    body: &Option<crate::transpile::hir::Block>,
    ret_type: &str,
) -> String {
    

    let body = match body {
        Some(b) => b,
        None => return "    unimplemented!();\n".to_string(),
    };

    let mut output = String::new();
    let stmts = &body.0;

    for (i, stmt) in stmts.iter().enumerate() {
        if let Some(tokens) = codegen.gen_stmt(stmt) {
            let stmt_str = tokens.to_string();
            let is_last_stmt = i == stmts.len() - 1;

            // Handle return statements: if function returns non-() but body has return,
            // and the return type wasn't declared, we need to handle it
            let stmt_str = if stmt_str.starts_with("return ") && is_last_stmt && ret_type != "()" {
                // Remove "return " prefix and trailing semicolon, just output the expression
                stmt_str["return ".len()..]
                    .trim_end_matches(';')
                    .trim()
                    .to_string()
            } else {
                stmt_str
            };

            // Handle multiline statements
            for line in stmt_str.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // For the last statement, don't add semicolon if it would make it return ()
                // Instead, just output the expression so Rust returns it implicitly
                let line_final =
                    if is_last_stmt && !trimmed.ends_with('{') && !trimmed.ends_with('}') {
                        // Check if this is an expression statement (not control flow)
                        if !trimmed.starts_with("if ")
                            && !trimmed.starts_with("while ")
                            && !trimmed.starts_with("for ")
                            && !trimmed.starts_with("loop ")
                            && !trimmed.starts_with("match ")
                            && !trimmed.starts_with("return ")
                            && !trimmed.ends_with(',')
                            && !trimmed.ends_with(";")
                        {
                            // This is an expression that should be returned
                            trimmed.to_string()
                        } else if trimmed.ends_with(';') {
                            trimmed.trim_end_matches(';').trim().to_string()
                        } else {
                            trimmed.to_string()
                        }
                    } else if trimmed.ends_with('}') || trimmed.ends_with('{') {
                        trimmed.to_string()
                    } else if trimmed.ends_with(';') {
                        trimmed.to_string()
                    } else {
                        format!("{};", trimmed)
                    };
                output.push_str(&format!("    {}\n", line_final));
            }
        }
    }
    output
}
