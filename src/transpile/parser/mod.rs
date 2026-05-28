//! TypeScript parser using oxc

pub mod types;

use anyhow::Result;
use std::path::Path;
use crate::transpile::hir as hir;

pub fn parse_source(source: &str, is_tsx: bool) -> Result<hir::Module> {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser as OxcParser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let mut source_type = SourceType::default().with_module(true).with_typescript(true);
    if is_tsx { source_type = source_type.with_jsx(true); }

    let ret = OxcParser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() { anyhow::bail!("Parse error: {:?}", ret.errors[0]); }

    let items: Vec<_> = ret.program.body.iter().filter_map(convert_stmt).collect();
    Ok(hir::Module { source: String::new(), items, types: std::collections::HashMap::new() })
}

pub fn parse_file(path: &Path) -> Result<hir::Module> {
    let source = std::fs::read_to_string(path)?;
    parse_source(&source, path.extension().and_then(|e| e.to_str()) == Some("tsx"))
}

pub struct TsParser;
impl TsParser {
    pub fn new() -> Self { Self }
    pub fn parse_source(&self, s: &str) -> Result<hir::Module> { parse_source(s, false) }
    pub fn parse_tsx(&self, s: &str) -> Result<hir::Module> { parse_source(s, true) }
    pub fn parse_file(&self, p: &Path) -> Result<hir::Module> { parse_file(p) }
}
impl Default for TsParser { fn default() -> Self { Self::new() } }

pub mod stmt { pub use super::convert_stmt as from_oxc; }
pub mod expr { pub use super::convert_expr as from_oxc; }
pub mod jsx { pub use super::convert_jsx_element as from_oxc; }

use oxc_ast::ast::*;

fn convert_stmt(stmt: &Statement) -> Option<hir::ModuleItem> {
    match stmt {
        Statement::FunctionDeclaration(func) => Some(hir::ModuleItem::Decl(hir::Decl::Function(convert_function(func)?))),
        Statement::VariableDeclaration(var) => Some(hir::ModuleItem::Decl(hir::Decl::Variable(hir::VariableDecl { name: var.declarations.iter().next()?.id.name.to_string(), kind: convert_var_kind(&var.kind), type_: None, init: None, decls: vec![] }))),
        Statement::ImportDeclaration(imp) => { let source = imp.source.value.to_string(); let specifiers: Vec<_> = imp.specifiers.iter().filter_map(|s| { let name = match s { ImportSpecifier::ImportDefaultSpecifier(i) => i.local.name.to_string(), ImportSpecifier::ImportNamedSpecifier(i) => i.local.name.to_string(), ImportSpecifier::ImportNamespaceSpecifier(i) => i.local.name.to_string() }; Some(hir::ImportSpecifier::Named { name, alias: None }) }).collect(); Some(hir::ModuleItem::Import(hir::ImportDecl { source, specifiers })) }
        Statement::ExportNamedDeclaration(exp) => Some(hir::ModuleItem::Export(hir::Export::Named { decl: None })),
        Statement::ExportDefaultDeclaration(_) => Some(hir::ModuleItem::Export(hir::Export::Default { expr: convert_expr(&exp.declaration?)? })),
        _ => None,
    }
}

fn convert_function(func: &Function) -> Option<hir::FunctionDecl> {
    let name = func.id.as_ref()?.name.to_string();
    let params: Vec<_> = func.params.items.iter().map(|p| hir::Param { name: p.pattern.name.as_ref().map(|n| n.name.to_string()).unwrap_or_default(), type_: None, default: None, optional: p.pattern.type_annotation.is_some(), pattern: None }).collect();
    let return_type = None;
    let body = func.body.as_ref().map(|b| hir::Block(b.body.iter().filter_map(convert_stmt_to_stmt).collect()));
    Some(hir::FunctionDecl { name, generics: vec![], params, return_type, body, is_async: func.r#async, is_generator: false, decorators: vec![] })
}

fn convert_stmt_to_stmt(stmt: &Statement) -> Option<hir::Stmt> {
    match stmt {
        Statement::ExpressionStatement(e) => Some(hir::Stmt::Expr { expr: convert_expr(&e.expression)? }),
        Statement::ReturnStatement(r) => Some(hir::Stmt::Return { arg: r.argument.as_ref().and_then(convert_expr) }),
        Statement::IfStatement(i) => Some(hir::Stmt::If { test: convert_expr(&i.test)?, consequent: Box::new(hir::Stmt::Block(hir::Block(i.consequent.body.iter().filter_map(convert_stmt_to_stmt).collect()))), alternate: i.alternate.as_ref().map(|a| Box::new(hir::Stmt::Block(hir::Block(a.body.iter().filter_map(convert_stmt_to_stmt).collect())))) }),
        Statement::BlockStatement(b) => Some(hir::Stmt::Block(hir::Block(b.body.iter().filter_map(convert_stmt_to_stmt).collect()))),
        Statement::WhileStatement(w) => Some(hir::Stmt::While { test: convert_expr(&w.test)?, body: Box::new(hir::Stmt::Block(hir::Block(w.body.body.iter().filter_map(convert_stmt_to_stmt).collect()))) }),
        Statement::ForStatement(f) => { let init = f.init.as_ref().map(|i| match i { ForStatementInit::Expression(e) => hir::ForInit::Expr(convert_expr(e)?), ForStatementInit::VariableDeclaration(v) => hir::ForInit::Variable(hir::VariableDecl { name: v.declarations.first()?.id.name.to_string(), kind: convert_var_kind(&v.kind), type_: None, init: None, decls: vec![] }), _ => return None }); Some(hir::Stmt::For { init, test: f.test.as_ref().and_then(convert_expr), update: f.update.as_ref().and_then(convert_expr), body: Box::new(hir::Stmt::Block(hir::Block(vec![]))) }) }
        Statement::BreakStatement(_) => Some(hir::Stmt::Break),
        Statement::ContinueStatement(_) => Some(hir::Stmt::Continue),
        Statement::SwitchStatement(s) => { let cases: Vec<_> = s.cases.iter().map(|c| hir::SwitchCase { test: c.test.as_ref().and_then(convert_expr), consequent: c.consequent.body.iter().filter_map(convert_stmt_to_stmt).collect() }).collect(); Some(hir::Stmt::Switch { discriminant: convert_expr(&s.discriminant)?, cases }) }
        Statement::TryStatement(t) => Some(hir::Stmt::Try { block: Box::new(hir::Stmt::Block(hir::Block(t.block.body.iter().filter_map(convert_stmt_to_stmt).collect()))), handler: t.handler.as_ref().map(|h| Box::new(hir::Stmt::Block(hir::Block(h.body.body.iter().filter_map(convert_stmt_to_stmt).collect())))), finalizer: t.finalizer.as_ref().map(|f| Box::new(hir::Stmt::Block(hir::Block(f.body.iter().filter_map(convert_stmt_to_stmt).collect()))) }) }),
        Statement::EmptyStatement(_) => None,
        Statement::DebuggerStatement(_) => None,
        _ => None,
    }
}

fn convert_var_kind(kind: &VariableDeclarationKind) -> hir::VariableKind {
    match kind { VariableDeclarationKind::Const => hir::VariableKind::Const, VariableDeclarationKind::Let => hir::VariableKind::Let, _ => hir::VariableKind::Var }
}

fn convert_expr(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        Expression::NumberLiteral(n) => Some(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        Expression::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        Expression::NullLiteral(_) => Some(hir::Expr::Null),
        Expression::UndefinedLiteral(_) => Some(hir::Expr::Undefined),
        Expression::ArrowFunctionExpression(a) => Some(hir::Expr::Arrow { params: a.params.items.iter().map(|p| hir::Param { name: p.pattern.name.as_ref().map(|n| n.name.to_string()).unwrap_or_default(), type_: None, default: None, optional: p.pattern.type_annotation.is_some(), pattern: None }).collect(), body: Box::new(hir::Stmt::Return { arg: a.body.as_ref().and_then(|b| convert_expr(b)) }), is_async: a.r#async }),
        Expression::FunctionExpression(f) => Some(hir::Expr::Function { decl: Box::new(convert_function(f)?) }),
        Expression::ArrayExpression(a) => Some(hir::Expr::Array { elems: a.elements.iter().map(|e| e.as_ref().and_then(convert_expr)).collect() }),
        Expression::ObjectExpression(o) => Some(hir::Expr::Object { props: o.properties.iter().filter_map(|p| match p { ObjectPropertyKind::ObjectProperty(p) => Some(hir::ObjectProp::Init { key: hir::PropKey::Ident(p.key.name.to_string()), value: Box::new(convert_expr(&p.value)?) }), ObjectPropertyKind::SpreadProperty(s) => Some(hir::ObjectProp::Spread { value: Box::new(convert_expr(&s.argument)?) }), _ => None }).collect() }),
        Expression::MemberExpression(m) => Some(hir::Expr::Member { object: Box::new(convert_expr(&m.object)?), property: Box::new(convert_expr(&m.property)?), computed: m.computed, optional: false }),
        Expression::CallExpression(c) => Some(hir::Expr::Call { callee: Box::new(hir::Callee::Expr(Box::new(convert_expr(&c.callee)?))), args: c.arguments.iter().map(|a| convert_expr(&a).unwrap_or(hir::Expr::Null)).collect(), optional: false }),
        Expression::BinaryExpression(b) => Some(hir::Expr::Bin { op: convert_bin_op(&b.operator), left: Box::new(convert_expr(&b.left)?), right: Box::new(convert_expr(&b.right)?) }),
        Expression::UnaryExpression(u) => Some(hir::Expr::Unary { op: convert_unary_op(&u.operator), arg: Box::new(convert_expr(&u.argument)?), prefix: u.prefix }),
        Expression::UpdateExpression(u) => Some(hir::Expr::Update { op: if u.operator == oxc_ast::ast::UpdateOperator::Increment { hir::UpdateOp::Increment } else { hir::UpdateOp::Decrement }, arg: Box::new(convert_expr(&u.argument)?), prefix: u.prefix }),
        Expression::LogicalExpression(l) => Some(hir::Expr::Logical { op: convert_logical_op(&l.operator), left: Box::new(convert_expr(&l.left)?), right: Box::new(convert_expr(&l.right)?) }),
        Expression::ConditionalExpression(c) => Some(hir::Expr::Cond { test: Box::new(convert_expr(&c.test)?), consequent: Box::new(convert_expr(&c.consequent)?), alternate: Box::new(convert_expr(&c.alternate)?) }),
        Expression::AssignmentExpression(a) => Some(hir::Expr::Assign { op: convert_assign_op(&a.operator), left: Box::new(convert_expr(&a.left)?), right: Box::new(convert_expr(&a.right)?) }),
        Expression::NewExpression(n) => Some(hir::Expr::New { callee: Box::new(convert_expr(&n.callee)?), args: n.arguments.as_ref()?.items.iter().map(|a| convert_expr(&a).unwrap_or(hir::Expr::Null)).collect() }),
        Expression::AwaitExpression(a) => Some(hir::Expr::Await { arg: Box::new(convert_expr(&a.argument)?) }),
        Expression::ThisExpression(_) => Some(hir::Expr::Ident { name: "this".to_string() }),
        Expression::YieldExpression(y) => Some(hir::Expr::Ident { name: "yield".to_string() }),
        Expression::TaggedTemplateExpression(t) => convert_expr(&t.tag),
        Expression::TemplateLiteral(t) => Some(hir::Expr::Template { parts: t.quasis.iter().map(|q| hir::TemplatePart::String(q.value.raw.to_string())).collect(), exprs: t.expressions.iter().map(|e| convert_expr(e).unwrap_or(hir::Expr::Null)).collect() }),
        Expression::JSXElement(e) => Some(hir::Expr::JSX(convert_jsx_element(e)?)),
        Expression::ParenthesizedExpression(p) => convert_expr(&p.expression),
        Expression::SequenceExpression(s) => s.expressions.last().and_then(convert_expr),
        Expression::TypeScriptTypeAnnotation(t) => convert_expr(&t.expression),
        _ => None,
    }
}

fn convert_bin_op(op: &oxc_ast::ast::BinaryOperator) -> hir::BinaryOp {
    match op { oxc_ast::ast::BinaryOperator::Add => hir::BinaryOp::Add, oxc_ast::ast::BinaryOperator::Subtract => hir::BinaryOp::Sub, oxc_ast::ast::BinaryOperator::Multiply => hir::BinaryOp::Mul, oxc_ast::ast::BinaryOperator::Divide => hir::BinaryOp::Div, oxc_ast::ast::BinaryOperator::Modulo => hir::BinaryOp::Mod, _ => hir::BinaryOp::Add }
}

fn convert_unary_op(op: &oxc_ast::ast::UnaryOperator) -> hir::UnaryOp {
    match op { oxc_ast::ast::UnaryOperator::Minus => hir::UnaryOp::Minus, oxc_ast::ast::UnaryOperator::Plus => hir::UnaryOp::Plus, oxc_ast::ast::UnaryOperator::Not => hir::UnaryOp::Not, _ => hir::UnaryOp::Not }
}

fn convert_logical_op(op: &oxc_ast::ast::LogicalOperator) -> hir::LogicalOp {
    match op { oxc_ast::ast::LogicalOperator::And => hir::LogicalOp::And, oxc_ast::ast::LogicalOperator::Or => hir::LogicalOp::Or, _ => hir::LogicalOp::And }
}

fn convert_assign_op(op: &oxc_ast::ast::AssignmentOperator) -> hir::AssignOp {
    match op { oxc_ast::ast::AssignmentOperator::Assign => hir::AssignOp::Assign, oxc_ast::ast::AssignmentOperator::AddAssign => hir::AssignOp::AddAssign, _ => hir::AssignOp::Assign }
}

fn convert_jsx_element(elem: &oxc_ast::ast::JSXElement) -> hir::JSXExpr {
    let name = match &elem.opening_element.name { oxc_ast::ast::JSXElementName::Identifier(id) => hir::JSXName::Ident(id.name.to_string()), oxc_ast::ast::JSXElementName::MemberExpression(m) => hir::JSXName::Member { object: m.object.name.to_string(), property: m.property.name.to_string() }, _ => hir::JSXName::Ident("div".to_string()) };
    let attrs: Vec<_> = elem.opening_element.attributes.iter().filter_map(|a| { let name = match &a.name { oxc_ast::ast::JSXAttributeName::Identifier(id) => id.name.to_string(), _ => return None }; let value = match &a.value { Some(oxc_ast::ast::JSXAttributeValue::ExpressionContainer(e)) => Some(hir::JSXAttrValue::Expr(Box::new(convert_expr(&e.expression)?.))), Some(oxc_ast::ast::JSXAttributeValue::StringLiteral(s)) => Some(hir::JSXAttrValue::String(s.value.to_string())), _ => None }; Some(hir::JSXAttr::Attr { name, value }) }).collect();
    let children: Vec<_> = elem.children.iter().filter_map(convert_jsx_child).collect();
    hir::JSXExpr { opening: hir::JSXOpening { name, attrs, self_closing: elem.opening_element.self_closing }, children, closing: elem.closing_element.as_ref().map(|c| hir::JSXClosing { name: hir::JSXName::Ident(c.name.name.to_string()) })) }
}

fn convert_jsx_child(child: &oxc_ast::ast::JSXChild) -> Option<hir::JSXChild> {
    match child { oxc_ast::ast::JSXChild::Text(t) => Some(hir::JSXChild::Text(t.value.to_string())), oxc_ast::ast::JSXChild::ExpressionContainer(e) => Some(hir::JSXChild::Expr(Box::new(convert_expr(&e.expression)?))), oxc_ast::ast::JSXChild::Element(el) => Some(hir::JSXChild::JSX(Box::new(convert_jsx_element(el)))), oxc_ast::ast::JSXChild::Fragment(f) => Some(hir::JSXChild::Fragment { children: f.children.iter().filter_map(convert_jsx_child).collect() }), _ => None }
}
