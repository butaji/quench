//! # Control Flow Emitters
//!
//! Emits Rust control flow statements (if, while, for, for-of).

use super::switch_match::emit_switch;
use super::code_emitter::TypeRegistry;
use super::{emit_expr, to_snake_case, CodeEmitter};
use swc_ecma_ast::{Expr, ForStmt, Pat, Stmt, VarDecl, VarDeclOrExpr};

/// Extract variable name from pattern.
pub fn extract_var_name(name: &Pat) -> Option<String> {
    if let Pat::Ident(ident) = name {
        Some(to_snake_case(ident.id.sym.as_ref()))
    } else {
        None
    }
}

/// Extract variable type from pattern.
pub fn extract_var_type(name: &Pat) -> String {
    if let Pat::Ident(ident) = name {
        if let Some(type_ann) = &ident.type_ann {
            resolve_type_to_rust(&type_ann.type_ann)
        } else {
            "i32".to_string()
        }
    } else {
        "i32".to_string()
    }
}

fn resolve_type_to_rust(ts_type: &swc_ecma_ast::TsType) -> String {
    match ts_type {
        swc_ecma_ast::TsType::TsKeywordType(k) => match k.kind {
            swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => "i32".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => "String".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => "bool".to_string(),
            _ => "()".to_string(),
        },
        swc_ecma_ast::TsType::TsArrayType(arr) => {
            let inner = resolve_type_to_rust(&arr.elem_type);
            format!("Vec<{inner}>")
        }
        _ => "i32".to_string(),
    }
}

/// Emit if statement.
pub fn emit_if_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::IfStmt) {
    emitter.push_str("if ");
    emit_expr(emitter, &stmt.test);
    emitter.push_str(" {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = &*stmt.cons {
        emit_block_stmts(emitter, block);
    } else {
        emitter.push_indent();
        emit_simple_stmt(emitter, &stmt.cons);
    }
    emitter.dec_indent();
    emitter.push_indent();
    if let Some(alt) = &stmt.alt {
        emitter.push_str("} else ");
        if let Stmt::If(else_if) = &**alt {
            emit_if_stmt(emitter, else_if);
        } else {
            emitter.push_str("{\n");
            emitter.inc_indent();
            if let Stmt::Block(block) = &**alt {
                emit_block_stmts(emitter, block);
            } else {
                emitter.push_indent();
                emit_simple_stmt(emitter, alt);
            }
            emitter.dec_indent();
            emitter.push_indent();
            emitter.push_str("}\n");
        }
    } else {
        emitter.push_str("}\n");
    }
}

/// Emit while statement.
pub fn emit_while_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::WhileStmt) {
    emitter.push_str("while ");
    emit_expr(emitter, &stmt.test);
    emitter.push_str(" {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = &*stmt.body {
        emit_block_stmts(emitter, block);
    } else {
        emit_single_stmt(emitter, &stmt.body);
    }
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit for statement.
pub fn emit_for_stmt(emitter: &mut CodeEmitter, stmt: &ForStmt) {
    if let Some((var_name, var_type, start, end)) = try_parse_counting_loop(stmt) {
        emit_counting_loop(emitter, &var_name, &var_type, &start, &end, &stmt.body);
    } else {
        emit_while_fallback_for_loop(emitter, stmt);
    }
}

fn emit_counting_loop(
    emitter: &mut CodeEmitter,
    var_name: &str,
    _var_type: &str,
    start: &str,
    end: &str,
    body: &Stmt,
) {
    emitter.push_str(&format!("for {var_name} in {start}..{end} {{\n"));
    emitter.inc_indent();
    if let Stmt::Block(block) = body {
        emit_block_stmts(emitter, block);
    } else {
        emit_single_stmt(emitter, body);
    }
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

fn emit_while_fallback_for_loop(emitter: &mut CodeEmitter, stmt: &ForStmt) {
    emit_for_init(emitter, stmt.init.as_ref());
    emit_while_with_update(emitter, stmt.test.as_deref(), &stmt.body, stmt.update.as_deref());
}

fn emit_for_init(emitter: &mut CodeEmitter, init: Option<&VarDeclOrExpr>) {
    let Some(init) = init else { return };
    match init {
        VarDeclOrExpr::VarDecl(d) => emit_var_init_list(emitter, d),
        VarDeclOrExpr::Expr(e) => emit_expr_init(emitter, e),
    }
}

fn emit_var_init_list(emitter: &mut CodeEmitter, var_decl: &VarDecl) {
    for decl in &var_decl.decls {
        let Some(name) = extract_var_name(&decl.name) else { continue };
        let ty = extract_var_type(&decl.name);
        if let Some(ref boxed_init) = decl.init {
            emit_var_declaration(emitter, &name, &ty, Some(boxed_init));
        } else {
            emit_var_declaration(emitter, &name, &ty, None);
        }
    }
}

fn emit_var_declaration(emitter: &mut CodeEmitter, name: &str, ty: &str, init: Option<&Expr>) {
    let Some(init_expr) = init else { return };
    let start_len = emitter.output().len();
    emit_expr(emitter, init_expr);
    let end_len = emitter.output().len();
    let init_str = emitter.output()[start_len..end_len].to_string();
    emitter.output_mut().truncate(start_len);
    emitter.push_str(&format!("let mut {name}: {ty} = {init_str};\n"));
}

fn emit_expr_init(emitter: &mut CodeEmitter, expr: &Expr) {
    emitter.push_indent();
    emit_expr(emitter, expr);
    emitter.push_str(";\n");
}

fn emit_while_with_update(emitter: &mut CodeEmitter, test: Option<&Expr>, body: &Stmt, update: Option<&Expr>) {
    emitter.push_str("while ");
    emit_loop_condition(emitter, test);
    emitter.push_str(" {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = body {
        emit_block_stmts(emitter, block);
    } else {
        emit_single_stmt(emitter, body);
    }
    emit_update_expr(emitter, update);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

fn emit_loop_condition(emitter: &mut CodeEmitter, test: Option<&Expr>) {
    if let Some(test_expr) = test {
        emit_expr(emitter, test_expr);
    } else {
        emitter.push_str("true");
    }
}

fn emit_update_expr(emitter: &mut CodeEmitter, update: Option<&Expr>) {
    let Some(update_expr) = update else { return };
    emitter.push_indent();
    emit_expr(emitter, update_expr);
    emitter.push_str(";\n");
}

fn try_parse_counting_loop(stmt: &ForStmt) -> Option<(String, String, String, String)> {
    let VarDeclOrExpr::VarDecl(var_decl) = stmt.init.as_ref()? else { return None };
    if var_decl.decls.len() != 1 {
        return None;
    }
    let decl = &var_decl.decls[0];
    let var_name = extract_var_name(&decl.name)?;
    let var_type = extract_var_type(&decl.name);
    let start_str = expr_to_string(decl.init.as_ref()?);
    let test_expr = stmt.test.as_ref()?;
    let (compare_var, op, limit_str) = extract_comparison(test_expr)?;
    if compare_var != var_name || op != "<" {
        return None;
    }
    let update_expr = stmt.update.as_ref()?;
    if !is_increment(update_expr, &var_name) {
        return None;
    }
    Some((var_name, var_type, start_str, limit_str))
}

fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match lit {
            swc_ecma_ast::Lit::Num(n) => {
                if n.value.fract() == 0.0 {
                    format!("{}i32", n.value as i64)
                } else {
                    n.value.to_string()
                }
            }
            swc_ecma_ast::Lit::BigInt(_) => "0i64".to_string(),
            swc_ecma_ast::Lit::Str(s) => format!("{:?}", s.value),
            swc_ecma_ast::Lit::Bool(b) => b.value.to_string(),
            _ => "0".to_string(),
        },
        Expr::Ident(ident) => to_snake_case(ident.sym.as_ref()),
        Expr::Member(member) => {
            let obj = expr_to_string(&member.obj);
            match &member.prop {
                swc_ecma_ast::MemberProp::Ident(ident) => {
                    let prop = to_snake_case(ident.sym.as_ref());
                    if prop == "length" {
                        format!("({obj}.len())")
                    } else {
                        format!("{obj}.{prop}")
                    }
                }
                swc_ecma_ast::MemberProp::Computed(comp) => {
                    format!("{obj}[{}]", expr_to_string(&comp.expr))
                }
                swc_ecma_ast::MemberProp::PrivateName(_) => obj,
            }
        }
        Expr::Bin(bin) => {
            let left = expr_to_string(&bin.left);
            let right = expr_to_string(&bin.right);
            let op = match bin.op {
                swc_ecma_ast::BinaryOp::Add => "+",
                swc_ecma_ast::BinaryOp::Sub => "-",
                swc_ecma_ast::BinaryOp::Mul => "*",
                swc_ecma_ast::BinaryOp::Div => "/",
                swc_ecma_ast::BinaryOp::Mod => "%",
                _ => "+",
            };
            format!("({left} {op} {right})")
        }
        _ => "0".to_string(),
    }
}

fn extract_comparison(expr: &Expr) -> Option<(String, &str, String)> {
    let Expr::Bin(bin) = expr else { return None };
    let op_str = match bin.op {
        swc_ecma_ast::BinaryOp::Lt => "<",
        swc_ecma_ast::BinaryOp::LtEq => "<=",
        swc_ecma_ast::BinaryOp::Gt => ">",
        swc_ecma_ast::BinaryOp::GtEq => ">=",
        _ => return None,
    };
    let (compare_var, compare_expr) = match (&*bin.left, &*bin.right) {
        (Expr::Ident(ident), right) => (ident.sym.to_string(), right),
        (left, Expr::Ident(ident)) => (ident.sym.to_string(), left),
        _ => return None,
    };
    Some((compare_var, op_str, expr_to_string(compare_expr)))
}

fn is_increment(expr: &Expr, var_name: &str) -> bool {
    match expr {
        Expr::Update(update) => {
            if let Expr::Ident(ident) = &*update.arg {
                ident.sym.as_ref() == var_name
                    && update.op == swc_ecma_ast::UpdateOp::PlusPlus
            } else {
                false
            }
        }
        Expr::Assign(assign) => {
            if let swc_ecma_ast::AssignTarget::Simple(
                swc_ecma_ast::SimpleAssignTarget::Ident(ident),
            ) = &assign.left
            {
                ident.id.sym.as_ref() == var_name
                    && matches!(
                        assign.op,
                        swc_ecma_ast::AssignOp::AddAssign | swc_ecma_ast::AssignOp::SubAssign
                    )
            } else {
                false
            }
        }
        _ => false,
    }
}

fn emit_block_stmts(emitter: &mut CodeEmitter, block: &swc_ecma_ast::BlockStmt) {
    for s in &block.stmts {
        emit_single_stmt(emitter, s);
    }
}

fn emit_simple_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Expr(expr_stmt) => {
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Decl(decl) => super::variables::emit_var_decl(emitter, decl),
        Stmt::Return(ret) => emit_return_simple(emitter, ret),
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str(";\n"),
    }
}

fn emit_return_simple(emitter: &mut CodeEmitter, ret: &swc_ecma_ast::ReturnStmt) {
    if let Some(arg) = &ret.arg {
        emitter.push_str("return ");
        emit_expr(emitter, arg);
        emitter.push_str(";\n");
    } else {
        emitter.push_str("return ();\n");
    }
}

pub fn emit_single_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    emitter.push_indent();
    match stmt {
        Stmt::Expr(expr_stmt) => emit_single_expr_stmt(emitter, expr_stmt),
        Stmt::Decl(decl) => super::variables::emit_var_decl(emitter, decl),
        Stmt::If(if_stmt) => emit_if_stmt(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while_stmt(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for_stmt(emitter, for_stmt),
        Stmt::ForOf(for_of_stmt) => emit_for_of_stmt(emitter, for_of_stmt),
        Stmt::Switch(switch_stmt) => emit_switch(emitter, switch_stmt),
        Stmt::Block(block) => emit_block_with_indent(emitter, block),
        Stmt::Return(ret) => emit_return_stmt(emitter, ret),
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str("// unsupported\n"),
    }
}

fn emit_single_expr_stmt(emitter: &mut CodeEmitter, expr_stmt: &swc_ecma_ast::ExprStmt) {
    emit_expr(emitter, &expr_stmt.expr);
    emitter.push_str(";\n");
}

fn emit_block_with_indent(emitter: &mut CodeEmitter, block: &swc_ecma_ast::BlockStmt) {
    emitter.push_str("{\n");
    emitter.inc_indent();
    for s in &block.stmts {
        emit_single_stmt(emitter, s);
    }
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

pub fn emit_return_stmt(emitter: &mut CodeEmitter, ret: &swc_ecma_ast::ReturnStmt) {
    emitter.push_indent();
    if let Some(arg) = &ret.arg {
        emit_return_with_value(emitter, arg);
    } else {
        emitter.push_str("return ();\n");
    }
}

fn emit_return_with_value(emitter: &mut CodeEmitter, arg: &Expr) {
    if let Some(expected) = emitter.expected_return() {
        if is_custom_struct_type(expected, &emitter.type_registry()) {
            let prev_struct = emitter.object_struct_name().cloned();
            emitter.set_object_struct(Some(expected.clone()));
            emitter.push_str("return ");
            emit_expr(emitter, arg);
            emitter.push_str(";\n");
            restore_struct_context(emitter, prev_struct);
            return;
        }
    }
    emitter.push_str("return ");
    emit_expr(emitter, arg);
    emitter.push_str(";\n");
}

/// Check if a type is a custom user-defined struct using the type registry.
///
/// This replaces the old starts_with("Vec") approach which would incorrectly
/// classify user types like "Vector3" as built-in containers.
fn is_custom_struct_type(ty: &str, registry: &TypeRegistry) -> bool {
    // First check if it's a known user-defined type via the registry
    if registry.is_user_defined_type(ty) {
        return true;
    }

    // Check for known built-in types
    if registry.is_builtin_type(ty) {
        return false;
    }

    // If it starts with an uppercase letter, it's likely a user-defined struct
    // (unless it was caught by the registry above)
    if let Some(c) = ty.chars().next() {
        c.is_uppercase()
    } else {
        false
    }
}

fn restore_struct_context(emitter: &mut CodeEmitter, prev_struct: Option<String>) {
    if let Some(prev) = prev_struct {
        emitter.set_object_struct(Some(prev));
    } else {
        emitter.set_object_struct(None);
    }
}

/// Emit for-of statement.
pub fn emit_for_of_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::ForOfStmt) {
    match &stmt.left {
        swc_ecma_ast::ForHead::VarDecl(var_decl) => {
            let var_name = extract_for_of_var_name(var_decl);
            if let Some(name) = &var_name {
                emit_for_of_loop(emitter, name, &stmt.right, &stmt.body);
            } else {
                emitter.push_str("// unsupported for-of pattern\n");
            }
        }
        swc_ecma_ast::ForHead::Pat(_pat) => emit_for_of_pattern(emitter, &stmt.right),
        swc_ecma_ast::ForHead::UsingDecl(_) => emitter.push_str("// using not supported\n"),
    }
}

fn extract_for_of_var_name(var_decl: &VarDecl) -> Option<String> {
    var_decl.decls.iter().find_map(|decl| extract_var_name(&decl.name))
}

fn emit_for_of_loop(emitter: &mut CodeEmitter, var_name: &str, right: &Expr, body: &Stmt) {
    let prev_struct = emitter.object_struct_name().cloned();
    emitter.set_object_struct(None);
    emitter.push_str(&format!("for {var_name} in "));
    emit_expr(emitter, right);
    emitter.push_str(".iter().cloned() {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = body {
        emit_block_stmts(emitter, block);
    } else {
        emit_single_stmt(emitter, body);
    }
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
    restore_struct_context(emitter, prev_struct);
}

fn emit_for_of_pattern(emitter: &mut CodeEmitter, right: &Expr) {
    emitter.push_str("// pattern: ");
    emit_expr(emitter, right);
    emitter.push_str(";\n");
}
