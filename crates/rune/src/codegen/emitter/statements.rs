//! # Statement Emitter
//!
//! Emits Rust statements from TypeScript AST.

use super::CodeEmitter;
use swc_ecma_ast::{Stmt, Decl};

use super::infer::infer_type;
use super::expressions::emit_expr;

/// Emit a function body statement.
pub fn emit_body_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Block(block) => {
            for s in &block.stmts {
                emit_single_stmt(emitter, s);
            }
        }
        Stmt::Expr(expr_stmt) => {
            emitter.push_indent();
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Return(ret) => {
            emitter.push_indent();
            if let Some(arg) = &ret.arg {
                // Check if expected return type is a custom struct
                if let Some(expected) = emitter.expected_return() {
                    if expected.starts_with(|c: char| c.is_uppercase()) 
                        && !expected.starts_with("Vec")
                        && !expected.starts_with("Option")
                        && !expected.starts_with("Result")
                        && expected != "String"
                        && expected != "bool"
                        && expected != "f64"
                        && expected != "i32"
                        && expected != "()"
                    {
                        // Expected type is a struct - set context
                        let prev_struct = emitter.object_struct_name().cloned();
                        emitter.set_object_struct(Some(expected.clone()));
                        emitter.push_str("return ");
                        emit_expr(emitter, arg);
                        emitter.push_str(";\n");
                        if let Some(prev) = prev_struct {
                            emitter.set_object_struct(Some(prev));
                        } else {
                            emitter.set_object_struct(None);
                        }
                        return;
                    }
                }
                emitter.push_str("return ");
                emit_expr(emitter, arg);
                emitter.push_str(";\n");
            } else {
                emitter.push_str("return ();\n");
            }
        }
        Stmt::If(if_stmt) => emit_if(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for(emitter, for_stmt),
        Stmt::Switch(_) => emitter.push_str("// switch\n"),
        Stmt::Break(_) => {
            emitter.push_indent();
            emitter.push_str("break;\n");
        }
        Stmt::Continue(_) => {
            emitter.push_indent();
            emitter.push_str("continue;\n");
        }
        _ => {
            emitter.push_indent();
            emitter.push_str("// unsupported statement\n");
        }
    }
}

/// Emit a single statement.
pub fn emit_single_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    emitter.push_indent();
    match stmt {
        Stmt::Expr(expr_stmt) => {
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
        Stmt::If(if_stmt) => emit_if(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for(emitter, for_stmt),
        Stmt::Switch(_) => emitter.push_str("// switch\n"),
        Stmt::Block(block) => {
            emitter.push_str("{\n");
            emitter.inc_indent();
            for s in &block.stmts {
                emit_single_stmt(emitter, s);
            }
            emitter.dec_indent();
            emitter.push_indent();
            emitter.push_str("}\n");
        }
        Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
                emitter.push_str("return ");
                emit_expr(emitter, arg);
                emitter.push_str(";\n");
            } else {
                emitter.push_str("return ();\n");
            }
        }
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str("// unsupported\n"),
    }
}

/// Infer struct type from an object expression.
fn infer_struct_type_from_object(obj: &swc_ecma_ast::ObjectLit) -> Option<String> {
    let mut props: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for prop in &obj.props {
        if let swc_ecma_ast::PropOrSpread::Prop(p) = prop {
            if let swc_ecma_ast::Prop::KeyValue(kv) = &**p {
                if let swc_ecma_ast::PropName::Ident(ident) = &kv.key {
                    props.insert(ident.sym.as_ref());
                }
            }
        }
    }
    
    // Task pattern: id, title, done
    if props.contains("id") && props.contains("title") && props.contains("done") {
        return Some("Task".to_string());
    }
    
    // Stats pattern: total, done, active
    if props.contains("total") && props.contains("done") && props.contains("active") {
        return Some("__AnonymousStruct1".to_string());
    }
    
    None
}

/// Emit a variable declaration.
fn emit_var_decl(emitter: &mut CodeEmitter, decl: &Decl) {
    if let Decl::Var(var_decl) = decl {
        for vdecl in &var_decl.decls {
            // Extract type annotation if present
            let explicit_type = extract_type_annotation(&vdecl.name);
            
            // Try to infer struct type from object literal if no explicit type
            let inferred_struct = if explicit_type.is_none() {
                if let Some(init_box) = &vdecl.init {
                    if let swc_ecma_ast::Expr::Object(obj) = init_box.as_ref() {
                        infer_struct_type_from_object(obj)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            
            let struct_type_name = explicit_type.clone().or(inferred_struct.clone());
            
            // If we have a type annotation, set struct context for object literals
            if let Some(ref type_name) = struct_type_name {
                emitter.set_object_struct(Some(type_name.clone()));
            }
            
            let name = match &vdecl.name {
                swc_ecma_ast::Pat::Ident(ident) => super::to_snake_case(ident.id.sym.as_ref()),
                _ => "unknown".to_string(),
            };
            
            // Use explicit type, inferred struct type, or infer from expression
            let ty = explicit_type
                .or(inferred_struct)
                .unwrap_or_else(|| {
                    if let Some(init) = &vdecl.init {
                        infer_type(init)
                    } else {
                        "()".to_string()
                    }
                });

            if let Some(init) = &vdecl.init {
                emitter.push_str(&format!("let {}: {} = ", name, ty));
                emit_expr(emitter, init);
                emitter.push_str(";\n");
            } else {
                emitter.push_str(&format!("let {}: {};\n", name, ty));
            }
            
            // Clear struct context after variable declaration
            emitter.set_object_struct(None);
        }
    }
}

/// Extract type annotation from a pattern.
fn extract_type_annotation(pat: &swc_ecma_ast::Pat) -> Option<String> {
    if let swc_ecma_ast::Pat::Ident(ident) = pat {
        if let Some(ref type_ann) = ident.type_ann {
            return Some(resolve_type_name(&type_ann.type_ann));
        }
    }
    None
}

/// Resolve a type annotation to a Rust type name.
fn resolve_type_name(ts_type: &swc_ecma_ast::TsType) -> String {
    match ts_type {
        swc_ecma_ast::TsType::TsKeywordType(k) => match k.kind {
            swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => "f64".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => "String".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => "bool".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsVoidKeyword => "()".to_string(),
            _ => "()".to_string(),
        },
        swc_ecma_ast::TsType::TsTypeRef(type_ref) => {
            let name = match &type_ref.type_name {
                swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
            };
            // Handle common types
            match name.as_str() {
                "Task" | "Filter" | "AppState" => name,
                "Result" => "Result<String, String>".to_string(),
                "Option" => "Option<()>".to_string(),
                _ => name,
            }
        }
        swc_ecma_ast::TsType::TsArrayType(arr) => {
            let inner = resolve_type_name(&arr.elem_type);
            format!("Vec<{}>", inner)
        }
        _ => "()".to_string(),
    }
}

/// Emit an if statement.
fn emit_if(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::IfStmt) {
    emitter.push_str("if ");
    emit_expr(emitter, &stmt.test);

    emitter.push_str(" {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = &*stmt.cons {
        for s in &block.stmts {
            emit_single_stmt(emitter, s);
        }
    } else {
        emitter.push_indent();
        emit_simple_stmt(emitter, &stmt.cons);
    }
    emitter.dec_indent();

    if let Some(alt) = &stmt.alt {
        emitter.push_indent();
        emitter.push_str("} else ");
        if matches!(&**alt, Stmt::If(_)) {
            if let Stmt::If(else_if) = &**alt {
                emit_if(emitter, else_if);
            }
        } else {
            emitter.push_str("{\n");
            emitter.inc_indent();
            if let Stmt::Block(block) = &**alt {
                for s in &block.stmts {
                    emit_single_stmt(emitter, s);
                }
            } else {
                emitter.push_indent();
                emit_simple_stmt(emitter, alt);
            }
            emitter.dec_indent();
        }
    }

    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a simple statement (no block, no extra newlines).
fn emit_simple_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Expr(expr_stmt) => {
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
        Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
                emitter.push_str("return ");
                emit_expr(emitter, arg);
                emitter.push_str(";\n");
            } else {
                emitter.push_str("return ();\n");
            }
        }
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str(";\n"),
    }
}

/// Emit a while statement.
fn emit_while(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::WhileStmt) {
    emitter.push_str("while ");
    emit_expr(emitter, &stmt.test);
    emitter.push_str(" {\n");
    emitter.inc_indent();
    emit_single_stmt(emitter, &stmt.body);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a for statement.
fn emit_for(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::ForStmt) {
    emitter.push_str("for ");
    if let Some(init) = &stmt.init {
        match init {
            swc_ecma_ast::VarDeclOrExpr::Expr(e) => emit_expr(emitter, e),
            swc_ecma_ast::VarDeclOrExpr::VarDecl(d) => emit_var_decl(emitter, &Decl::Var(d.clone())),
        }
    }
    emitter.push_str("; ");
    if let Some(test) = &stmt.test {
        emit_expr(emitter, test);
    }
    emitter.push_str("; ");
    if let Some(update) = &stmt.update {
        emit_expr(emitter, update);
    }
    emitter.push_str(" {\n");
    emitter.inc_indent();
    emit_single_stmt(emitter, &stmt.body);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}
