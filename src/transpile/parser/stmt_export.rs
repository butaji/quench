//! Export statement conversion

use crate::transpile::hir;
use crate::transpile::parser::expr::{arr_elems, convert_expr, conv_arrow, conv_object, conv_template};

use oxc_ast::ast::*;

pub fn export_default_kind_to_expr(kind: &ExportDefaultDeclarationKind) -> Option<hir::Expr> {
    if let Some(n) = kind.as_numeric_literal() { return Some(hir::Expr::Number(n.value)); }
    if let Some(s) = kind.as_string_literal() { return Some(hir::Expr::String(s.value.to_string())); }
    if let Some(b) = kind.as_boolean_literal() { return Some(hir::Expr::Boolean(b.value)); }
    if kind.as_null_literal().is_some() { return Some(hir::Expr::Null); }
    if let Some(id) = kind.as_identifier() { return Some(hir::Expr::Ident { name: id.name.to_string() }); }
    export_default_kind_to_expr_impl(kind)
}

fn export_default_kind_to_expr_impl(kind: &ExportDefaultDeclarationKind) -> Option<hir::Expr> {
    if let Some(a) = kind.as_arrow_function_expression() { return conv_arrow(a); }
    if let Some(t) = kind.as_template_literal() { return conv_template(t).ok(); }
    if let Some(o) = kind.as_object_expression() { return conv_object(o).ok(); }
    if let Some(a) = kind.as_array_expression() { return Some(hir::Expr::Array { elems: arr_elems(a) }); }
    export_default_kind_to_expr_impl2(kind)
}

fn export_default_kind_to_expr_impl2(kind: &ExportDefaultDeclarationKind) -> Option<hir::Expr> {
    if let Some(c) = kind.as_class_expression() { return convert_class_expr(c); }
    if let Some(f) = kind.as_function_expression() { return convert_func_expr(f); }
    if let Some(c) = kind.as_call_expression() { return convert_call_expr(c); }
    if let Some(m) = kind.as_static_member_expression() { return convert_static_member(m); }
    if let Some(m) = kind.as_computed_member_expression() { return convert_computed_member(m); }
    export_default_kind_to_expr_impl3(kind)
}

fn export_default_kind_to_expr_impl3(kind: &ExportDefaultDeclarationKind) -> Option<hir::Expr> {
    if let Some(b) = kind.as_binary_expression() { return convert_binary_expr(b); }
    if let Some(u) = kind.as_unary_expression() { return convert_unary_expr(u); }
    if let Some(u) = kind.as_update_expression() { return convert_update_expr(u); }
    if let Some(c) = kind.as_conditional_expression() { return convert_conditional_expr(c); }
    if let Some(p) = kind.as_parenthesized_expression() { return convert_expr(&p.expression).ok(); }
    if let Some(b) = kind.as_bigint_literal() { return convert_bigint(b); }
    if let Some(r) = kind.as_regexp_literal() { return convert_regexp(r); }
    None
}

fn convert_class_expr(c: &ClassExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Class {
        id: c.id.as_ref().map(|i| i.name.to_string()),
        super_class: c.super_class.as_ref().and_then(|s| convert_expr(s).ok()).map(Box::new),
        members: vec![],
    })
}

fn convert_func_expr(f: &FunctionExpression) -> Option<hir::Expr> {
    let decl = func_to_decl(f);
    if let hir::Decl::Function(func_decl) = decl {
        Some(hir::Expr::Function(func_decl))
    } else {
        None
    }
}

fn convert_call_expr(c: &CallExpression) -> Option<hir::Expr> {
    let callee = Box::new(convert_expr(&c.callee).ok()?);
    let args: Vec<hir::Expr> = c.arguments.iter()
        .filter_map(|a| a.as_expression().and_then(|e| convert_expr(e).ok()))
        .collect();
    Some(hir::Expr::Call { callee, arguments: args })
}

fn convert_static_member(m: &StaticMemberExpression) -> Option<hir::Expr> {
    Some(hir::Expr::StaticMember {
        obj: Box::new(convert_expr(&m.object).ok()?),
        property: m.property.name.to_string(),
    })
}

fn convert_computed_member(m: &ComputedMemberExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Member {
        obj: Box::new(convert_expr(&m.object).ok()?),
        property: Box::new(convert_expr(&m.expression).ok()?),
        computed: true,
    })
}

fn convert_conditional_expr(c: &ConditionalExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Cond {
        test: Box::new(convert_expr(&c.test).ok()?),
        consequent: Box::new(convert_expr(&c.consequent).ok()?),
        alternate: Box::new(convert_expr(&c.alternate).ok()?),
    })
}

fn convert_bigint(b: &BigIntLiteral) -> Option<hir::Expr> {
    Some(hir::Expr::BigInt(
        b.raw.as_ref().map(|s| s.to_string())
            .unwrap_or_else(|| b.value.to_string())
            .parse().unwrap_or(0)
    ))
}

fn convert_regexp(r: &RegExpLiteral) -> Option<hir::Expr> {
    Some(hir::Expr::RegExp {
        pattern: r.regex.pattern.text.to_string(),
        flags: r.regex.flags.to_string(),
    })
}

fn convert_binary_expr(b: &BinaryExpression) -> Option<hir::Expr> {
    let left = Box::new(convert_expr(&b.left).ok()?);
    let right = Box::new(convert_expr(&b.right).ok()?);
    let op = binary_op_to_hir(&b.operator)?;
    Some(hir::Expr::Bin { op, left, right })
}

fn binary_op_to_hir(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    if let Some(bop) = arith_bin_op(op) { return Some(bop); }
    if let Some(bop) = cmp_bin_op(op) { return Some(bop); }
    if let Some(bop) = eq_bin_op(op) { return Some(bop); }
    if let Some(bop) = shift_bin_op(op) { return Some(bop); }
    bit_bin_op(op)
}

fn arith_bin_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    match op {
        BinaryOperator::Addition => Some(hir::BinaryOp::Add),
        BinaryOperator::Subtraction => Some(hir::BinaryOp::Sub),
        BinaryOperator::Multiplication => Some(hir::BinaryOp::Mul),
        BinaryOperator::Division => Some(hir::BinaryOp::Div),
        BinaryOperator::Remainder => Some(hir::BinaryOp::Mod),
        BinaryOperator::Exponential => Some(hir::BinaryOp::Exp),
        _ => None,
    }
}

fn cmp_bin_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    match op {
        BinaryOperator::LessThan => Some(hir::BinaryOp::Lt),
        BinaryOperator::LessEqualThan => Some(hir::BinaryOp::Lte),
        BinaryOperator::GreaterThan => Some(hir::BinaryOp::Gt),
        BinaryOperator::GreaterEqualThan => Some(hir::BinaryOp::Gte),
        BinaryOperator::In => Some(hir::BinaryOp::In),
        BinaryOperator::Instanceof => Some(hir::BinaryOp::Instanceof),
        _ => None,
    }
}

fn eq_bin_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    match op {
        BinaryOperator::Equality => Some(hir::BinaryOp::Eq),
        BinaryOperator::StrictEquality => Some(hir::BinaryOp::StrictEq),
        BinaryOperator::Inequality => Some(hir::BinaryOp::Neq),
        BinaryOperator::StrictInequality => Some(hir::BinaryOp::StrictNeq),
        _ => None,
    }
}

fn shift_bin_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    match op {
        BinaryOperator::ShiftLeft => Some(hir::BinaryOp::Shl),
        BinaryOperator::ShiftRight => Some(hir::BinaryOp::Shr),
        BinaryOperator::ShiftRightZeroFill => Some(hir::BinaryOp::UShr),
        _ => None,
    }
}

fn bit_bin_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    match op {
        BinaryOperator::BitwiseAnd => Some(hir::BinaryOp::BitAnd),
        BinaryOperator::BitwiseXOR => Some(hir::BinaryOp::BitXor),
        BinaryOperator::BitwiseOR => Some(hir::BinaryOp::BitOr),
        _ => None,
    }
}

fn convert_unary_expr(u: &UnaryExpression) -> Option<hir::Expr> {
    let op = match u.operator {
        UnaryOperator::UnaryNegation => hir::UnaryOp::Minus,
        UnaryOperator::UnaryPlus => hir::UnaryOp::Plus,
        UnaryOperator::LogicalNot => hir::UnaryOp::Not,
        UnaryOperator::BitwiseNot => hir::UnaryOp::BitNot,
        UnaryOperator::Typeof => hir::UnaryOp::Typeof,
        UnaryOperator::Void => hir::UnaryOp::Void,
        UnaryOperator::Delete => hir::UnaryOp::Delete,
    };
    Some(hir::Expr::Unary { op, arg: Box::new(convert_expr(&u.argument).ok()?), prefix: true })
}

fn convert_update_expr(u: &UpdateExpression) -> Option<hir::Expr> {
    let op = match u.operator {
        UpdateOperator::Increment => hir::UpdateOp::PlusPlus,
        UpdateOperator::Decrement => hir::UpdateOp::MinusMinus,
    };
    let arg = match &u.argument {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => hir::Expr::Ident { name: id.name.to_string() },
        _ => return None,
    };
    Some(hir::Expr::Update { op, arg: Box::new(arg), prefix: u.prefix })
}

pub fn convert_export_named(e: &ExportNamedDeclaration) -> Vec<hir::ModuleItem> {
    if let Some(source) = &e.source {
        return convert_reexport(source, &e.specifiers);
    }
    if let Some(declaration) = &e.declaration {
        return convert_export_decl(declaration);
    }
    convert_local_exports(&e.specifiers)
}

fn convert_reexport(source: &Str, specifiers: &[ExportSpecifier]) -> Vec<hir::ModuleItem> {
    let source_str = source.value.to_string();
    if specifiers.is_empty() {
        return vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed {
            specifiers: vec![hir::Export::All { source: source_str }],
        })];
    }
    let names: Vec<String> = specifiers.iter().map(|spec| module_export_name(&spec.exported)).collect();
    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed {
        specifiers: vec![hir::Export::ReExport { source: source_str, names }],
    })]
}

fn convert_export_decl(declaration: &oxc_ast::ast::Declaration) -> Vec<hir::ModuleItem> {
    match declaration {
        oxc_ast::ast::Declaration::VariableDeclaration(v) => {
            var_to_decl(v).into_iter().map(hir::ModuleItem::Decl).collect()
        }
        oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
            vec![hir::ModuleItem::Decl(func_to_decl(f))]
        }
        oxc_ast::ast::Declaration::ClassDeclaration(c) => {
            vec![hir::ModuleItem::Decl(class_to_hir(c))]
        }
        oxc_ast::ast::Declaration::TSInterfaceDeclaration(i) => {
            vec![hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
                name: i.id.name.to_string(),
                generics: vec![],
                type_: hir::Type::Object { members: vec![] },
            }))]
        }
        _ => vec![],
    }
}

fn convert_local_exports(specifiers: &[ExportSpecifier]) -> Vec<hir::ModuleItem> {
    let mut specs = Vec::new();
    for spec in specifiers {
        specs.push(hir::Export::Named { name: module_export_name(&spec.exported) });
    }
    if specs.is_empty() { vec![] } else { vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed { specifiers: specs })] }
}

fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::IdentifierName(i) => i.name.to_string(),
        ModuleExportName::IdentifierReference(i) => i.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

pub fn convert_module_item(stmt: &Statement) -> Vec<hir::ModuleItem> {
    if let Some(item) = try_class_expr_module_item(stmt) { return item; }
    if let Some(item) = try_decl_module_item(stmt) { return item; }
    if let Some(item) = try_import_module_item(stmt) { return item; }
    if let Some(item) = try_interface_module_item(stmt) { return item; }
    if let Some(item) = try_export_module_item(stmt) { return item; }
    vec![hir::ModuleItem::Stmt(stmt_to_hir_stmt(stmt))]
}

fn try_decl_module_item(stmt: &Statement) -> Option<Vec<hir::ModuleItem>> {
    match stmt {
        Statement::ClassDeclaration(c) => Some(vec![hir::ModuleItem::Decl(class_to_hir(c))]),
        Statement::FunctionDeclaration(f) => Some(vec![hir::ModuleItem::Decl(func_to_decl(f))]),
        Statement::VariableDeclaration(v) => Some(var_to_decl(v).into_iter().map(hir::ModuleItem::Decl).collect()),
        _ => None,
    }
}

fn try_import_module_item(stmt: &Statement) -> Option<Vec<hir::ModuleItem>> {
    let Statement::ImportDeclaration(i) = stmt else { return None; };
    Some(vec![import_to_hir(i)])
}

fn try_interface_module_item(stmt: &Statement) -> Option<Vec<hir::ModuleItem>> {
    let Statement::TSInterfaceDeclaration(i) = stmt else { return None; };
    Some(make_type_module_item(&i.id.name))
}

fn try_export_module_item(stmt: &Statement) -> Option<Vec<hir::ModuleItem>> {
    match stmt {
        Statement::ExportDefaultDeclaration(e) => Some(convert_export_default(e)),
        Statement::ExportNamedDeclaration(e) => Some(convert_export_named(e)),
        Statement::ExportAllDeclaration(e) => Some(vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed {
            specifiers: vec![hir::Export::All { source: e.source.value.to_string() }],
        })]),
        _ => None,
    }
}

fn try_class_expr_module_item(stmt: &Statement) -> Option<Vec<hir::ModuleItem>> {
    let Statement::VariableDeclaration(v) = stmt else { return None; };
    let Some(decl) = v.declarations.first() else { return None; };
    let BindingPattern::BindingIdentifier(_id) = &decl.id else { return None; };
    let Some(init) = &decl.init else { return None; };
    let Expression::ClassExpression(c) = init else { return None; };
    Some(vec![hir::ModuleItem::Decl(class_to_hir(c))])
}

fn make_type_module_item(name: &str) -> Vec<hir::ModuleItem> {
    vec![hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
        name: name.to_string(), generics: vec![], type_: hir::Type::Object { members: vec![] },
    }))]
}

fn convert_export_default(e: &ExportDefaultDeclaration) -> Vec<hir::ModuleItem> {
    match &e.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(_) | ExportDefaultDeclarationKind::ClassDeclaration(_) | ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {
            convert_export_decl_or_type(e)
        }
        ExportDefaultDeclarationKind::NumericLiteral(n) => expr_stmt(hir::Expr::Number(n.value)),
        ExportDefaultDeclarationKind::StringLiteral(s) => expr_stmt(hir::Expr::String(s.value.to_string())),
        ExportDefaultDeclarationKind::BooleanLiteral(b) => expr_stmt(hir::Expr::Boolean(b.value)),
        ExportDefaultDeclarationKind::NullLiteral(_) => expr_stmt(hir::Expr::Null),
        ExportDefaultDeclarationKind::Identifier(id) => expr_stmt(hir::Expr::Ident { name: id.name.to_string() }),
        ExportDefaultDeclarationKind::ArrowFunctionExpression(a) => convert_arrow_export(a),
        _ => convert_default_kind(e),
    }
}

fn convert_export_decl_or_type(e: &ExportDefaultDeclaration) -> Vec<hir::ModuleItem> {
    match &e.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(f) => export_func_decl(f),
        ExportDefaultDeclarationKind::ClassDeclaration(c) => export_class_decl(c),
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(i) => make_type_module_item(&i.id.name),
        _ => vec![],
    }
}

fn export_func_decl(f: &oxc_index::IndexNewtype<ts_interface::OxcHandle<ts_interface::ast::FunctionDeclaration>>) -> Vec<hir::ModuleItem> {
    vec![hir::ModuleItem::Decl(func_to_decl(f))]
}

fn export_class_decl(c: &oxc_index::IndexNewtype<ts_interface::OxcHandle<ts_interface::ast::ClassDeclaration>>) -> Vec<hir::ModuleItem> {
    vec![hir::ModuleItem::Decl(class_to_hir(c))]
}

fn convert_arrow_export(a: &oxc_index::IndexNewtype<ts_interface::OxcHandle<ts_interface::ast::ArrowFunctionExpression>>) -> Vec<hir::ModuleItem> {
    if let Some(expr) = conv_arrow(a) {
        vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault { expr })]
    } else {
        vec![hir::ModuleItem::Stmt(hir::Stmt::Empty)]
    }
}

fn convert_default_kind(e: &ExportDefaultDeclaration) -> Vec<hir::ModuleItem> {
    let expr = export_default_kind_to_expr(&e.declaration).unwrap_or(hir::Expr::Undefined);
    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault { expr })]
}

fn expr_stmt(expr: hir::Expr) -> Vec<hir::ModuleItem> {
    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault { expr })]
}

// Re-use functions from other modules
use super::stmt_convert::{class_to_hir, stmt_to_hir_stmt};
use super::stmt_decl::{func_to_decl, import_to_hir, var_to_decl};
