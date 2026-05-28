//! TypeScript parser using oxc
//!
//! Converts TypeScript/JavaScript to our HIR for compilation to Rust.

pub mod types;

use anyhow::Result;
use std::path::Path;
use crate::transpile::hir as hir;

/// Parse source code to HIR
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

/// Parse a file
pub fn parse_file(path: &Path) -> Result<hir::Module> {
    let source = std::fs::read_to_string(path)?;
    parse_source(&source, path.extension().and_then(|e| e.to_str()) == Some("tsx"))
}

/// Parser struct
pub struct TsParser;
impl TsParser {
    pub fn new() -> Self { Self }
    pub fn parse_source(&self, s: &str) -> Result<hir::Module> { parse_source(s, false) }
    pub fn parse_tsx(&self, s: &str) -> Result<hir::Module> { parse_source(s, true) }
    pub fn parse_file(&self, p: &Path) -> Result<hir::Module> { parse_file(p) }
}
impl Default for TsParser { fn default() -> Self { Self::new() } }

fn convert_stmt(stmt: &oxc_ast::ast::Statement) -> Option<hir::ModuleItem> {
    use oxc_ast::ast::*;

    match stmt {
        Statement::FunctionDeclaration(func) => {
            let name = func.id.as_ref()?.name.to_string();
            let params: Vec<_> = (0..func.params.items.len())
                .map(|i| hir::Param {
                    name: format!("arg{}", i),
                    type_: None,
                    optional: false,
                    default: None,
                    pattern: None,
                }).collect();
            let body = func.body.as_ref()
                .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
                .unwrap_or_default();
            let decl = hir::FunctionDecl {
                name, generics: vec![], params, return_type: None,
                body: Some(body), is_async: func.r#async,
                is_generator: func.generator, decorators: vec![],
            };
            Some(hir::ModuleItem::Export(hir::Export::NamedWithValue {
                name: decl.name.clone(),
                value: hir::Expr::Function { decl },
            }))
        }
        Statement::VariableDeclaration(var_decl) => {
            if let Some((i, decl)) = var_decl.declarations.iter().enumerate().next() {
                let name = format!("var{}", i);
                let init = decl.init.as_ref().and_then(convert_expr);
                return Some(hir::ModuleItem::Export(hir::Export::NamedWithValue {
                    name,
                    value: init.unwrap_or(hir::Expr::Null),
                }));
            }
            None
        }
        Statement::ImportDeclaration(decl) => {
            let src = decl.source.value.to_string();
            let specs: Vec<_> = decl.specifiers.as_ref().map_or(vec![], |specs| {
                specs.iter().filter_map(|s| match s {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        Some(hir::ImportSpecifier::Named { name: s.local.name.to_string(), alias: None })
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        Some(hir::ImportSpecifier::Default { name: s.local.name.to_string() })
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        Some(hir::ImportSpecifier::Namespace { name: s.local.name.to_string() })
                    },
                }).collect()
            });
            let type_only = matches!(decl.import_kind, ImportOrExportKind::Type);
            Some(hir::ModuleItem::Import(hir::Import { source: src, specifiers: specs, type_only }))
        }
        Statement::ExportNamedDeclaration(decl) => {
            if let Some(d) = &decl.declaration {
                match d {
                    Declaration::FunctionDeclaration(func) => {
                        let name = func.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
                        let params: Vec<_> = (0..func.params.items.len())
                            .map(|i| hir::Param {
                                name: format!("arg{}", i),
                                type_: None, optional: false,
                                default: None,
                                pattern: None,
                            }).collect();
                        let body = func.body.as_ref()
                            .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
                            .unwrap_or_default();
                        let decl = hir::FunctionDecl {
                            name: name.clone(), generics: vec![], params, return_type: None,
                            body: Some(body), is_async: func.r#async,
                            is_generator: func.generator, decorators: vec![],
                        };
                        return Some(hir::ModuleItem::Export(hir::Export::NamedWithValue {
                            name,
                            value: hir::Expr::Function { decl },
                        }));
                    }
                    Declaration::VariableDeclaration(var) => {
                        if let Some((i, v)) = var.declarations.iter().enumerate().next() {
                            let name = format!("var{}", i);
                            let init = v.init.as_ref().and_then(convert_expr);
                            return Some(hir::ModuleItem::Export(hir::Export::NamedWithValue {
                                name,
                                value: init.unwrap_or(hir::Expr::Null),
                            }));
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        Statement::ExportDefaultDeclaration(_decl) => {
            // Export default needs special handling - return empty for now
            None
        }
        _ => None,
    }
}

fn convert_stmt_to_stmt(stmt: &oxc_ast::ast::Statement) -> Option<hir::Stmt> {
    use oxc_ast::ast::*;

    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            convert_expr(&expr_stmt.expression).map(|expr| hir::Stmt::Expr { expr })
        }
        Statement::ReturnStatement(ret) => {
            Some(hir::Stmt::Return { arg: ret.argument.as_ref().and_then(convert_expr) })
        }
        Statement::VariableDeclaration(var_decl) => {
            if let Some(decl) = var_decl.declarations.first() {
                let name = "var".to_string();
                let init = decl.init.as_ref().and_then(convert_expr);
                return Some(hir::Stmt::Variable { decl: hir::VariableDecl {
                    name,
                    kind: match var_decl.kind {
                        VariableDeclarationKind::Const => hir::VariableKind::Const,
                        VariableDeclarationKind::Let => hir::VariableKind::Let,
                        _ => hir::VariableKind::Var,
                    },
                    type_: None,
                    init,
                    pattern: None,
                }});
            }
            Some(hir::Stmt::Empty)
        }
        Statement::BlockStatement(block) => {
            Some(hir::Stmt::Block(
                block.body.iter().filter_map(convert_stmt_to_stmt).collect()
            ))
        }
        Statement::BreakStatement(_) => Some(hir::Stmt::Break { label: None }),
        Statement::ContinueStatement(_) => Some(hir::Stmt::Continue { label: None }),
        Statement::EmptyStatement(_) => None,
        Statement::FunctionDeclaration(func) => {
            let name = func.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
            let params: Vec<_> = (0..func.params.items.len())
                .map(|i| hir::Param {
                    name: format!("arg{}", i),
                    type_: None, optional: false,
                    default: None,
                    pattern: None,
                }).collect();
            let body = func.body.as_ref()
                .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
                .unwrap_or_default();
            Some(hir::Stmt::Function { decl: hir::FunctionDecl {
                name, generics: vec![], params, return_type: None,
                body: Some(body), is_async: func.r#async,
                is_generator: func.generator, decorators: vec![],
            }})
        }
        _ => None,
    }
}

fn convert_expr(expr: &oxc_ast::ast::Expression) -> Option<hir::Expr> {
    use oxc_ast::ast::*;
    use oxc_syntax::operator::{BinaryOperator, UnaryOperator, LogicalOperator};

    match expr {
        Expression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        Expression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        Expression::NullLiteral(_) => Some(hir::Expr::Null),
        Expression::BigIntLiteral(n) => n.value.parse().ok().map(hir::Expr::BigInt),
        Expression::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        Expression::ThisExpression(_) => Some(hir::Expr::Null),
        Expression::Super(_) => Some(hir::Expr::Null),
        
        Expression::ArrayExpression(_arr) => {
            // Simplified: return empty array since matching ArrayExpressionElement is complex
            Some(hir::Expr::Array { elems: vec![] })
        }
        
        Expression::ObjectExpression(obj) => {
            let props: Vec<_> = obj.properties.iter().filter_map(|prop| {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let key_name = p.key.name().map(|n| n.to_string()).unwrap_or_default();
                    let key = hir::PropKey::Ident(key_name);
                    Some(hir::ObjectProp::Init { key, value: hir::Expr::Null })
                } else { None }
            }).collect();
            Some(hir::Expr::Object { props })
        }
        
        Expression::BinaryExpression(bin) => {
            let left = convert_expr(&bin.left)?;
            let right = convert_expr(&bin.right)?;
            let op = match bin.operator {
                BinaryOperator::Addition => hir::BinaryOp::Add,
                BinaryOperator::Subtraction => hir::BinaryOp::Sub,
                BinaryOperator::Multiplication => hir::BinaryOp::Mul,
                BinaryOperator::Division => hir::BinaryOp::Div,
                BinaryOperator::Remainder => hir::BinaryOp::Mod,
                BinaryOperator::Equality => hir::BinaryOp::Eq,
                BinaryOperator::StrictEquality => hir::BinaryOp::EqStrict,
                BinaryOperator::Inequality => hir::BinaryOp::Ne,
                BinaryOperator::StrictInequality => hir::BinaryOp::NeStrict,
                BinaryOperator::LessThan => hir::BinaryOp::Lt,
                BinaryOperator::LessEqualThan => hir::BinaryOp::Le,
                BinaryOperator::GreaterThan => hir::BinaryOp::Gt,
                BinaryOperator::GreaterEqualThan => hir::BinaryOp::Ge,
                BinaryOperator::BitwiseAnd => hir::BinaryOp::BitAnd,
                BinaryOperator::BitwiseOR => hir::BinaryOp::BitOr,
                BinaryOperator::ShiftLeft => hir::BinaryOp::LeftShift,
                BinaryOperator::ShiftRight => hir::BinaryOp::RightShift,
                BinaryOperator::In => hir::BinaryOp::In,
                BinaryOperator::Instanceof => hir::BinaryOp::InstanceOf,
                _ => hir::BinaryOp::Add,
            };
            Some(hir::Expr::Bin { op, left: Box::new(left), right: Box::new(right) })
        }
        
        Expression::UnaryExpression(unary) => {
            let arg = convert_expr(&unary.argument)?;
            let op = match unary.operator {
                UnaryOperator::UnaryNegation => hir::UnaryOp::Minus,
                UnaryOperator::UnaryPlus => hir::UnaryOp::Plus,
                UnaryOperator::LogicalNot => hir::UnaryOp::Not,
                UnaryOperator::BitwiseNot => hir::UnaryOp::BitNot,
                UnaryOperator::Typeof => hir::UnaryOp::TypeOf,
                UnaryOperator::Void => hir::UnaryOp::Void,
                _ => hir::UnaryOp::Minus,
            };
            Some(hir::Expr::Unary { op, arg: Box::new(arg), prefix: true })
        }
        
        Expression::LogicalExpression(logical) => {
            let left = convert_expr(&logical.left)?;
            let right = convert_expr(&logical.right)?;
            let op = match logical.operator {
                LogicalOperator::And => hir::LogicalOp::And,
                LogicalOperator::Or => hir::LogicalOp::Or,
                LogicalOperator::Coalesce => hir::LogicalOp::NullishCoalesce,
            };
            Some(hir::Expr::Logical { op, left: Box::new(left), right: Box::new(right) })
        }
        
        Expression::ConditionalExpression(cond) => {
            let test = convert_expr(&cond.test)?;
            let consequent = convert_expr(&cond.consequent)?;
            let alternate = convert_expr(&cond.alternate)?;
            Some(hir::Expr::Cond { test: Box::new(test), consequent: Box::new(consequent), alternate: Box::new(alternate) })
        }
        
        Expression::CallExpression(call) => {
            let callee = convert_expr(&call.callee)?;
            let args: Vec<_> = call.arguments.iter().filter_map(|arg| {
                arg.as_expression().and_then(convert_expr)
            }).collect();
            Some(hir::Expr::Call { callee: Box::new(callee), args, type_args: vec![] })
        }
        
        Expression::StaticMemberExpression(member) => {
            let object = convert_expr(&member.object)?;
            let property_name = member.property.name.to_string();
            let property = hir::Expr::Ident { name: property_name };
            Some(hir::Expr::Member { object: Box::new(object), property: Box::new(property), computed: false, optional: member.optional })
        }
        
        Expression::NewExpression(new) => {
            let callee = convert_expr(&new.callee)?;
            // Simplified: empty args for new expressions
            Some(hir::Expr::New { callee: Box::new(callee), args: vec![], type_args: vec![] })
        }
        
        Expression::ArrowFunctionExpression(arrow) => {
            let params: Vec<_> = (0..arrow.params.items.len())
                .map(|i| hir::Param {
                    name: format!("arg{}", i),
                    type_: None, optional: false,
                    default: None,
                    pattern: None,
                }).collect();
            let body = hir::Stmt::Return { arg: None };
            Some(hir::Expr::Arrow { params, body: Box::new(body), is_async: arrow.r#async })
        }
        
        Expression::FunctionExpression(func) => {
            let name = func.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
            let params: Vec<_> = (0..func.params.items.len())
                .map(|i| hir::Param {
                    name: format!("arg{}", i),
                    type_: None, optional: false,
                    default: None,
                    pattern: None,
                }).collect();
            let body = func.body.as_ref()
                .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
                .unwrap_or_default();
            Some(hir::Expr::Function { decl: hir::FunctionDecl {
                name, generics: vec![], params, return_type: None,
                body: Some(body), is_async: func.r#async,
                is_generator: func.generator, decorators: vec![],
            }})
        }
        
        Expression::TemplateLiteral(lit) => {
            let parts: Vec<_> = lit.quasis.iter().map(|q| hir::TemplatePart::String(q.value.raw.to_string())).collect();
            let exprs: Vec<_> = lit.expressions.iter().filter_map(convert_expr).collect();
            Some(hir::Expr::Template { parts, exprs })
        }
        
        Expression::AssignmentExpression(_assign) => {
            // Simplified: just return null for assignments
            Some(hir::Expr::Null)
        }
        
        Expression::SequenceExpression(seq) => {
            let exprs: Vec<_> = seq.expressions.iter().filter_map(convert_expr).collect();
            Some(hir::Expr::Seq { exprs })
        }
        
        Expression::AwaitExpression(a) => convert_expr(&a.argument).map(|arg| hir::Expr::Await { arg: Box::new(arg) }),
        _ => None,
    }
}
