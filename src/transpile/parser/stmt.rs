//! Statement conversion
// allow:complexity

use crate::transpile::hir;
use crate::transpile::parser::expr::{
    arr_elems, convert_expr, conv_arrow, conv_object, conv_template,
    conv_assignment_target, convert_binding_pattern,
};
use oxc_ast::ast::*;

fn var_to_decl(v: &VariableDeclaration) -> Vec<hir::Decl> {
    let kind = match v.kind {
        VariableDeclarationKind::Const => hir::VariableKind::Const,
        VariableDeclarationKind::Let => hir::VariableKind::Let,
        VariableDeclarationKind::Var => hir::VariableKind::Var,
        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => hir::VariableKind::Var,
    };
    v.declarations
        .iter()
        .filter_map(|decl| {
            // Handle both identifier and destructuring patterns
            let (name, pattern) = match &decl.id {
                BindingPattern::BindingIdentifier(i) => (i.name.to_string(), None),
                BindingPattern::ArrayPattern(a) => {
                    let pat = convert_binding_pattern(&decl.id)?;
                    (String::new(), Some(pat))
                }
                BindingPattern::ObjectPattern(o) => {
                    let pat = convert_binding_pattern(&decl.id)?;
                    (String::new(), Some(pat))
                }
            };
            let init = decl.init.as_ref().and_then(|e| convert_expr(e).ok());
            Some(hir::Decl::Variable(hir::VariableDecl {
                name,
                kind: kind.clone(),
                type_: None,
                init,
                pattern,
            }))
        })
        .collect()
}

pub fn func_to_decl(f: &Function) -> hir::Decl {
    let params: Vec<hir::Param> = f
        .params
        .items
        .iter()
        .filter_map(|p| {
            if let BindingPattern::BindingIdentifier(i) = &p.pattern {
                Some(hir::Param {
                    name: i.name.to_string(),
                    type_: None,
                    default: None,
                    optional: p.optional,
                    pattern: None,
                    ownership: hir::Ownership::Owned,
                })
            } else {
                // Handle destructuring patterns
                let pattern = convert_binding_pattern(&p.pattern)?;
                Some(hir::Param {
                    name: String::new(),
                    type_: None,
                    default: None,
                    optional: p.optional,
                    pattern: Some(pattern),
                    ownership: hir::Ownership::Owned,
                })
            }
        })
        .collect();

    let body = f.body.as_ref().map(|body_box| {
        hir::Block(body_box.statements.iter().map(stmt_to_hir_stmt).collect())
    });

    hir::Decl::Function(hir::FunctionDecl {
        name: f
            .id
            .as_ref()
            .map(|i| i.name.to_string())
            .unwrap_or_default(),
        generics: vec![],
        params,
        return_type: None,
        body,
        is_async: f.r#async,
        is_generator: false,
        decorators: vec![],
        throws: false,
        error_type: None,
    })
}

fn import_to_hir(i: &ImportDeclaration) -> hir::ModuleItem {
    let specs = i.specifiers.as_ref().map_or(vec![], |v| {
        v.iter()
            .map(|s| match s {
                oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    hir::ImportSpecifier::Named {
                        name: s.local.name.to_string(),
                        alias: None,
                    }
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    hir::ImportSpecifier::Default {
                        name: s.local.name.to_string(),
                    }
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    hir::ImportSpecifier::Namespace {
                        name: s.local.name.to_string(),
                    }
                }
            })
            .collect()
    });
    hir::ModuleItem::Import(hir::Import {
        source: i.source.value.to_string(),
        specifiers: specs,
        type_only: false,
    })
}

fn stmt_to_hir_stmt(s: &Statement) -> hir::Stmt {
    match s {
        Statement::ExpressionStatement(e) => hir::Stmt::Expr {
            expr: convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined),
        },
        Statement::IfStatement(stmt) => hir::Stmt::If {
            test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
            consequent: Box::new(stmt_to_hir_stmt(&stmt.consequent)),
            alternate: stmt
                .alternate
                .as_ref()
                .map(|a| Box::new(stmt_to_hir_stmt(a))),
        },
        Statement::WhileStatement(stmt) => hir::Stmt::While {
            test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
            body: Box::new(stmt_to_hir_stmt(&stmt.body)),
        },
        Statement::ForStatement(stmt) => {
            // ForStatementInit is Option<ForStatementInit> where ForStatementInit
            // has VariableDeclaration variant plus inherited Expression variants
            let init = match &stmt.init {
                Some(ForStatementInit::VariableDeclaration(v)) => {
                    let kind = match v.kind {
                        VariableDeclarationKind::Const => hir::VariableKind::Const,
                        VariableDeclarationKind::Let => hir::VariableKind::Let,
                        VariableDeclarationKind::Var => hir::VariableKind::Var,
                        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => hir::VariableKind::Var,
                    };
                    let vars: Vec<(String, Option<hir::Expr>)> = v
                        .declarations
                        .iter()
                        .filter_map(|d| {
                            let name = match &d.id {
                                BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                                _ => return None,
                            };
                            let init = d.init.as_ref().and_then(|e| convert_expr(e).ok());
                            Some((name, init))
                        })
                        .collect();
                    Some(hir::ForInit::Variable(kind, vars))
                }
                _ => {
                    // Handle expression-based for init (e.g., for (i = 0; ...))
                    stmt.init.as_ref().and_then(|i| {
                        i.as_expression().and_then(|e| {
                            convert_expr(e).ok().map(|e| hir::ForInit::Expr(Box::new(e)))
                        })
                    })
                }
            };
            hir::Stmt::For {
                init,
                test: stmt.test.as_ref().and_then(|t| convert_expr(t).ok()),
                update: stmt.update.as_ref().and_then(|u| convert_expr(u).ok()),
                body: Box::new(stmt_to_hir_stmt(&stmt.body)),
            }
        }
        Statement::ForInStatement(stmt) => {
            let left = match &stmt.left {
                ForStatementLeft::VariableDeclaration(v) => {
                    let kind = match v.kind {
                        VariableDeclarationKind::Const => hir::VariableKind::Const,
                        VariableDeclarationKind::Let => hir::VariableKind::Let,
                        VariableDeclarationKind::Var => hir::VariableKind::Var,
                        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => hir::VariableKind::Var,
                    };
                    let vars: Vec<(String, Option<hir::Expr>)> = v
                        .declarations
                        .iter()
                        .filter_map(|d| {
                            let name = match &d.id {
                                BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                                _ => return None,
                            };
                            Some((name, d.init.as_ref().and_then(|e| convert_expr(e).ok())))
                        })
                        .collect();
                    hir::ForInit::Variable(kind, vars)
                }
                _ => {
                    // Handle expression-based for init (e.g., for (x.y in obj))
                    // ForStatementLeft can be AssignmentTargetIdentifier or MemberExpressions
                    match &stmt.left {
                        ForStatementLeft::AssignmentTargetIdentifier(id) => {
                            hir::ForInit::Expr(Box::new(hir::Expr::Ident { name: id.name.to_string() }))
                        }
                        ForStatementLeft::ComputedMemberExpression(m) => {
                            conv_assignment_target(&AssignmentTarget::ComputedMemberExpression(
                                Box::new(m.clone())
                            )).unwrap_or(hir::ForInit::Variable(hir::VariableKind::Let, vec![]))
                        }
                        ForStatementLeft::StaticMemberExpression(m) => {
                            conv_assignment_target(&AssignmentTarget::StaticMemberExpression(
                                Box::new(m.clone())
                            )).unwrap_or(hir::ForInit::Variable(hir::VariableKind::Let, vec![]))
                        }
                        ForStatementLeft::PrivateFieldExpression(m) => {
                            conv_assignment_target(&AssignmentTarget::PrivateFieldExpression(
                                Box::new(m.clone())
                            )).unwrap_or(hir::ForInit::Variable(hir::VariableKind::Let, vec![]))
                        }
                        _ => hir::ForInit::Variable(hir::VariableKind::Let, vec![]),
                    }
                }
            };
            hir::Stmt::ForIn {
                left,
                right: convert_expr(&stmt.right).unwrap_or(hir::Expr::Undefined),
                body: Box::new(stmt_to_hir_stmt(&stmt.body)),
            }
        }
        Statement::ForOfStatement(stmt) => {
            let left = match &stmt.left {
                ForStatementLeft::VariableDeclaration(v) => {
                    let kind = match v.kind {
                        VariableDeclarationKind::Const => hir::VariableKind::Const,
                        VariableDeclarationKind::Let => hir::VariableKind::Let,
                        VariableDeclarationKind::Var => hir::VariableKind::Var,
                        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => hir::VariableKind::Var,
                    };
                    let vars: Vec<(String, Option<hir::Expr>)> = v
                        .declarations
                        .iter()
                        .filter_map(|d| {
                            let name = match &d.id {
                                BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                                _ => return None,
                            };
                            Some((name, d.init.as_ref().and_then(|e| convert_expr(e).ok())))
                        })
                        .collect();
                    hir::ForInit::Variable(kind, vars)
                }
                _ => {
                    // Handle expression-based for init (e.g., for (x.y of arr))
                    match &stmt.left {
                        ForStatementLeft::AssignmentTargetIdentifier(id) => {
                            hir::ForInit::Expr(Box::new(hir::Expr::Ident { name: id.name.to_string() }))
                        }
                        ForStatementLeft::ComputedMemberExpression(m) => {
                            conv_assignment_target(&AssignmentTarget::ComputedMemberExpression(
                                Box::new(m.clone())
                            )).unwrap_or(hir::ForInit::Variable(hir::VariableKind::Let, vec![]))
                        }
                        ForStatementLeft::StaticMemberExpression(m) => {
                            conv_assignment_target(&AssignmentTarget::StaticMemberExpression(
                                Box::new(m.clone())
                            )).unwrap_or(hir::ForInit::Variable(hir::VariableKind::Let, vec![]))
                        }
                        ForStatementLeft::PrivateFieldExpression(m) => {
                            conv_assignment_target(&AssignmentTarget::PrivateFieldExpression(
                                Box::new(m.clone())
                            )).unwrap_or(hir::ForInit::Variable(hir::VariableKind::Let, vec![]))
                        }
                        _ => hir::ForInit::Variable(hir::VariableKind::Let, vec![]),
                    }
                }
            };
            hir::Stmt::ForOf {
                left,
                right: convert_expr(&stmt.right).unwrap_or(hir::Expr::Undefined),
                body: Box::new(stmt_to_hir_stmt(&stmt.body)),
                is_await: stmt.r#await,
            }
        }
        Statement::DoWhileStatement(stmt) => hir::Stmt::DoWhile {
            body: Box::new(stmt_to_hir_stmt(&stmt.body)),
            test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
        },
        Statement::SwitchStatement(stmt) => {
            let discriminant = convert_expr(&stmt.discriminant).unwrap_or(hir::Expr::Undefined);
            let cases = stmt.cases.iter().map(|c| {
                hir::SwitchCase {
                    test: c.test.as_ref().map(|t| convert_expr(t).unwrap_or(hir::Expr::Undefined)),
                    consequent: c.consequent.iter().map(stmt_to_hir_stmt).collect(),
                }
            }).collect();
            hir::Stmt::Switch { discriminant, cases }
        }
        Statement::TryStatement(stmt) => {
            let handler = stmt.handler.as_ref().map(|h| {
                hir::CatchClause {
                    param: h.param.as_ref().map(|p| {
                        match &p.pattern {
                            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                            _ => String::new(),
                        }
                    }).unwrap_or_default(),
                    body: Box::new(hir::Block(h.body.body.iter().map(stmt_to_hir_stmt).collect())),
                }
            });
            hir::Stmt::Try {
                block: hir::Block(stmt.block.body.iter().map(stmt_to_hir_stmt).collect()),
                handler,
                finalizer: stmt.finalizer.as_ref().map(|f| hir::Block(f.body.iter().map(stmt_to_hir_stmt).collect())),
            }
        }
        Statement::ThrowStatement(stmt) => hir::Stmt::Throw {
            arg: convert_expr(&stmt.argument).unwrap_or(hir::Expr::Undefined),
        },
        Statement::WithStatement(stmt) => hir::Stmt::With {
            obj: convert_expr(&stmt.object).unwrap_or(hir::Expr::Undefined),
            body: Box::new(stmt_to_hir_stmt(&stmt.body)),
        },
        Statement::LabeledStatement(stmt) => hir::Stmt::Labeled {
            label: stmt.label.name.to_string(),
            body: Box::new(stmt_to_hir_stmt(&stmt.body)),
        },
        Statement::DebuggerStatement(_) => hir::Stmt::Empty,
        Statement::ReturnStatement(r) => hir::Stmt::Return {
            arg: r.argument.as_ref().and_then(|a| convert_expr(a).ok()),
        },
        Statement::BreakStatement(_) => hir::Stmt::Break { label: None },
        Statement::ContinueStatement(_) => hir::Stmt::Continue { label: None },
        Statement::BlockStatement(b) => {
            hir::Stmt::Block(b.body.iter().map(stmt_to_hir_stmt).collect())
        }
        Statement::EmptyStatement(_) => hir::Stmt::Empty,
        Statement::VariableDeclaration(v) => {
            // Convert to a Block with assignments
            let mut stmts = vec![];
            for decl in &v.declarations {
                let name = match &decl.id {
                    BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                    _ => continue,
                };
                let init = decl.init.as_ref().and_then(|e| convert_expr(e).ok());
                // Create: name = init
                let assign = hir::Expr::Assign {
                    op: hir::AssignOp::Assign,
                    left: Box::new(hir::Expr::Ident { name: name.clone() }),
                    right: Box::new(init.unwrap_or(hir::Expr::Undefined)),
                };
                stmts.push(hir::Stmt::Expr { expr: assign });
            }
            hir::Stmt::Block(stmts)
        }
        Statement::TSEnumDeclaration(_) => hir::Stmt::Empty,
        Statement::TSTypeAliasDeclaration(_) => hir::Stmt::Empty,
        Statement::TSExportAssignment(_) => hir::Stmt::Empty,
        Statement::ClassDeclaration(c) => {
            if let hir::Decl::Class(class_decl) = class_to_hir(c) {
                hir::Stmt::Class(class_decl)
            } else {
                hir::Stmt::Empty
            }
        }
        Statement::FunctionDeclaration(f) => {
            if let hir::Decl::Function(func_decl) = func_to_decl(f) {
                hir::Stmt::FunctionDecl(func_decl)
            } else {
                hir::Stmt::Empty
            }
        }
        Statement::ImportDeclaration(_) => hir::Stmt::Empty,
        Statement::ExportNamedDeclaration(_) => hir::Stmt::Empty,
        Statement::ExportDefaultDeclaration(_) => hir::Stmt::Empty,
        Statement::ExportAllDeclaration(_) => hir::Stmt::Empty,
        Statement::TSInterfaceDeclaration(_) => hir::Stmt::Empty,
        Statement::TSModuleDeclaration(_) => hir::Stmt::Empty,
        Statement::TSImportEqualsDeclaration(_) => hir::Stmt::Empty,
        Statement::ExpressionStatement(_) |
        Statement::IfStatement(_) |
        Statement::WhileStatement(_) |
        Statement::ForStatement(_) |
        Statement::ForInStatement(_) |
        Statement::ForOfStatement(_) |
        Statement::DoWhileStatement(_) |
        Statement::SwitchStatement(_) |
        Statement::TryStatement(_) |
        Statement::ThrowStatement(_) |
        Statement::WithStatement(_) |
        Statement::LabeledStatement(_) |
        Statement::DebuggerStatement(_) |
        Statement::ReturnStatement(_) |
        Statement::BreakStatement(_) |
        Statement::ContinueStatement(_) |
        Statement::BlockStatement(_) |
        Statement::EmptyStatement(_) |
        Statement::VariableDeclaration(_) |
        Statement::TSEnumDeclaration(_) |
        Statement::TSTypeAliasDeclaration(_) |
        Statement::TSExportAssignment(_) |
        Statement::UsingDeclaration(_) |
        Statement::ClassDeclaration(_) |
        Statement::FunctionDeclaration(_) |
        Statement::ImportDeclaration(_) |
        Statement::ExportNamedDeclaration(_) |
        Statement::ExportDefaultDeclaration(_) |
        Statement::ExportAllDeclaration(_) |
        Statement::TSInterfaceDeclaration(_) |
        Statement::TSModuleDeclaration(_) |
        Statement::TSImportEqualsDeclaration(_) => {
            // Statement types that are already handled above
            hir::Stmt::Empty
        }
    }
}

fn class_to_hir(c: &Class) -> hir::Decl {
    let methods: Vec<hir::ClassMethod> = c
        .body
        .body
        .iter()
        .filter_map(|m| {
            if let ClassElement::MethodDefinition(def) = m {
                let name = match &def.key {
                    PropertyKey::StaticIdentifier(i) => i.name.to_string(),
                    PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
                    _ => String::new(),
                };
                // def.value is a Function struct
                let func = &*def.value;
                let body = if let Some(body_box) = &func.body {
                    // Extract expression from first statement
                    if let Some(stmt) = body_box.statements.first() {
                        match stmt {
                            Statement::ExpressionStatement(e) => {
                                convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined)
                            }
                            Statement::ReturnStatement(r) => r
                                .argument
                                .as_ref()
                                .and_then(|a| convert_expr(a).ok())
                                .unwrap_or(hir::Expr::Undefined),
                            _ => hir::Expr::Undefined,
                        }
                    } else {
                        hir::Expr::Undefined
                    }
                } else {
                    hir::Expr::Undefined
                };
                let params: Vec<hir::Param> = func
                    .params
                    .items
                    .iter()
                    .filter_map(|param| {
                        if let BindingPattern::BindingIdentifier(i) = &param.pattern {
                            Some(hir::Param {
                                name: i.name.to_string(),
                                type_: None,
                                default: None,
                                optional: param.optional,
                                pattern: None,
                                ownership: hir::Ownership::Owned,
                            })
                        } else {
                            // Handle destructuring patterns
                            if let Some(pattern) = convert_binding_pattern(&param.pattern) {
                                Some(hir::Param {
                                    name: String::new(),
                                    type_: None,
                                    default: None,
                                    optional: param.optional,
                                    pattern: Some(pattern),
                                    ownership: hir::Ownership::Owned,
                                })
                            } else {
                                None
                            }
                        }
                    })
                    .collect();
                Some(hir::ClassMethod {
                    name,
                    params,
                    body,
                    kind: hir::MethodKind::Method,
                })
            } else {
                None
            }
        })
        .collect();
    hir::Decl::Class(hir::ClassDecl {
        name: c
            .id
            .as_ref()
            .map(|i| i.name.to_string())
            .unwrap_or_default(),
        extends: None,
        members: vec![],
        generics: vec![],
        methods,
    })
}

/// Convert ExportDefaultDeclarationKind to hir::Expr for export default expressions.
/// This handles the common inherited Expression variants from ExportDefaultDeclarationKind.
fn export_default_kind_to_expr(kind: &ExportDefaultDeclarationKind) -> Option<hir::Expr> {
    match kind {
        ExportDefaultDeclarationKind::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        ExportDefaultDeclarationKind::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        ExportDefaultDeclarationKind::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        ExportDefaultDeclarationKind::NullLiteral(_) => Some(hir::Expr::Null),
        ExportDefaultDeclarationKind::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        ExportDefaultDeclarationKind::ArrowFunctionExpression(a) => conv_arrow(a),
        ExportDefaultDeclarationKind::TemplateLiteral(t) => conv_template(t).ok(),
        ExportDefaultDeclarationKind::ObjectExpression(o) => conv_object(o).ok(),
        ExportDefaultDeclarationKind::ArrayExpression(a) => Some(hir::Expr::Array { elems: arr_elems(a) }),
        ExportDefaultDeclarationKind::ClassExpression(c) => Some(hir::Expr::Class {
            id: c.id.as_ref().map(|i| i.name.to_string()),
            super_class: c.super_class.as_ref().and_then(|s| convert_expr(s).ok()).map(Box::new),
            members: vec![],
        }),
        ExportDefaultDeclarationKind::FunctionExpression(f) => {
            // Convert Function to FunctionDecl, then use as Expr::Function
            let decl = func_to_decl(f);
            if let hir::Decl::Function(func_decl) = decl {
                Some(hir::Expr::Function(func_decl))
            } else {
                None
            }
        }
        // For complex expressions that need similar conversion logic,
        // delegate to the convert_expr function via Expression boxing
        ExportDefaultDeclarationKind::CallExpression(c) => {
            // Build a call expression from the parts
            let callee = Box::new(convert_expr(&c.callee).ok()?);
            let args: Vec<hir::Expr> = c
                .arguments
                .iter()
                .filter_map(|a| a.as_expression().and_then(|e| convert_expr(e).ok()))
                .collect();
            Some(hir::Expr::Call { callee, arguments: args })
        }
        ExportDefaultDeclarationKind::StaticMemberExpression(m) => {
            let obj = Box::new(convert_expr(&m.object).ok()?);
            Some(hir::Expr::StaticMember {
                obj,
                property: m.property.name.to_string(),
            })
        }
        ExportDefaultDeclarationKind::ComputedMemberExpression(m) => {
            let obj = Box::new(convert_expr(&m.object).ok()?);
            let property = Box::new(convert_expr(&m.expression).ok()?);
            Some(hir::Expr::Member {
                obj,
                property,
                computed: true,
            })
        }
        ExportDefaultDeclarationKind::BinaryExpression(b) => {
            let left = Box::new(convert_expr(&b.left).ok()?);
            let right = Box::new(convert_expr(&b.right).ok()?);
            let op = match b.operator {
                BinaryOperator::Addition => hir::BinaryOp::Add,
                BinaryOperator::Subtraction => hir::BinaryOp::Sub,
                BinaryOperator::Multiplication => hir::BinaryOp::Mul,
                BinaryOperator::Division => hir::BinaryOp::Div,
                BinaryOperator::Remainder => hir::BinaryOp::Mod,
                BinaryOperator::LessThan => hir::BinaryOp::Lt,
                BinaryOperator::LessEqualThan => hir::BinaryOp::Lte,
                BinaryOperator::GreaterThan => hir::BinaryOp::Gt,
                BinaryOperator::GreaterEqualThan => hir::BinaryOp::Gte,
                BinaryOperator::Equality => hir::BinaryOp::Eq,
                BinaryOperator::StrictEquality => hir::BinaryOp::StrictEq,
                BinaryOperator::Inequality => hir::BinaryOp::Neq,
                BinaryOperator::StrictInequality => hir::BinaryOp::StrictNeq,
                BinaryOperator::Exponential => hir::BinaryOp::Exp,
                BinaryOperator::ShiftLeft => hir::BinaryOp::Shl,
                BinaryOperator::ShiftRight => hir::BinaryOp::Shr,
                BinaryOperator::ShiftRightZeroFill => hir::BinaryOp::UShr,
                BinaryOperator::BitwiseAnd => hir::BinaryOp::BitAnd,
                BinaryOperator::BitwiseXOR => hir::BinaryOp::BitXor,
                BinaryOperator::BitwiseOR => hir::BinaryOp::BitOr,
                BinaryOperator::In => hir::BinaryOp::In,
                BinaryOperator::Instanceof => hir::BinaryOp::Instanceof,
                _ => return None,
            };
            Some(hir::Expr::Bin { op, left, right })
        }
        ExportDefaultDeclarationKind::UnaryExpression(u) => {
            let op = match u.operator {
                UnaryOperator::UnaryNegation => hir::UnaryOp::Minus,
                UnaryOperator::UnaryPlus => hir::UnaryOp::Plus,
                UnaryOperator::LogicalNot => hir::UnaryOp::Not,
                UnaryOperator::BitwiseNot => hir::UnaryOp::BitNot,
                UnaryOperator::Typeof => hir::UnaryOp::Typeof,
                UnaryOperator::Void => hir::UnaryOp::Void,
                UnaryOperator::Delete => hir::UnaryOp::Delete,
            };
            Some(hir::Expr::Unary {
                op,
                arg: Box::new(convert_expr(&u.argument).ok()?),
                prefix: true,
            })
        }
        ExportDefaultDeclarationKind::UpdateExpression(u) => {
            let op = match u.operator {
                UpdateOperator::Increment => hir::UpdateOp::PlusPlus,
                UpdateOperator::Decrement => hir::UpdateOp::MinusMinus,
            };
            // Convert the argument to a simple assignment target
            let arg = match &u.argument {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                    hir::Expr::Ident { name: id.name.to_string() }
                }
                _ => return None,
            };
            Some(hir::Expr::Update {
                op,
                arg: Box::new(arg),
                prefix: u.prefix,
            })
        }
        ExportDefaultDeclarationKind::ConditionalExpression(c) => {
            Some(hir::Expr::Cond {
                test: Box::new(convert_expr(&c.test).ok()?),
                consequent: Box::new(convert_expr(&c.consequent).ok()?),
                alternate: Box::new(convert_expr(&c.alternate).ok()?),
            })
        }
        ExportDefaultDeclarationKind::ParenthesizedExpression(p) => convert_expr(&p.expression).ok(),
        // Handle BigIntLiteral, RegExpLiteral, etc.
        ExportDefaultDeclarationKind::BigIntLiteral(b) => Some(hir::Expr::BigInt(b.raw.as_ref().map(|s| s.to_string()).unwrap_or_else(|| b.value.to_string()).parse().unwrap_or(0))),
        ExportDefaultDeclarationKind::RegExpLiteral(r) => Some(hir::Expr::RegExp {
            pattern: r.regex.pattern.text.to_string(),
            flags: r.regex.flags.to_string(),
        }),
        // Fall-through for unhandled types
        _ => None,
    }
}

/// Convert ExportNamedDeclaration to HIR ModuleItems.
/// Handles: export const x = 1; export function foo() {} export { x, y }; export * from "mod";
fn convert_export_named(e: &ExportNamedDeclaration) -> Vec<hir::ModuleItem> {
    use oxc_ast::ast::ExportSpecifier;

    // Handle re-exports: export * from "mod" or export { x, y } from "mod"
    if let Some(source) = &e.source {
        let source_str = source.value.to_string();
        // export * from "mod"
        if e.specifiers.is_empty() {
            return vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed {
                specifiers: vec![hir::Export::All {
                    source: source_str,
                }],
            })];
        }
        // export { x, y } from "mod"
        let names: Vec<String> = e
            .specifiers
            .iter()
            .map(|spec| {
                spec.exported
                    .as_ref()
                    .map(|i| i.name.to_string())
                    .unwrap_or_else(|| {
                        // Use local name if exported is not specified
                        spec.local.name.to_string()
                    })
            })
            .collect();
        return vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed {
            specifiers: vec![hir::Export::ReExport {
                source: source_str,
                names,
            }],
        })];
    }

    // Handle export declarations
    if let Some(declaration) = &e.declaration {
        match declaration {
            // export const x = 1; export let y = 2;
            oxc_ast::ast::Declaration::VariableDeclaration(v) => {
                var_to_decl(v)
                    .into_iter()
                    .map(|d| hir::ModuleItem::Decl(d))
                    .collect()
            }
            // export function foo() {}
            oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
                vec![hir::ModuleItem::Decl(func_to_decl(f))]
            }
            // export class Foo {}
            oxc_ast::ast::Declaration::ClassDeclaration(c) => {
                vec![hir::ModuleItem::Decl(class_to_hir(c))]
            }
            // export type Foo = ...
            oxc_ast::ast::Declaration::TSInterfaceDeclaration(i) => {
                vec![hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
                    name: i.id.name.to_string(),
                    generics: vec![],
                    type_: hir::Type::Object { members: vec![] },
                }))]
            }
            // Handle other declaration types
            _ => vec![],
        }
    } else {
        // Handle export { x, y } without source (local exports)
        let mut specifiers = Vec::new();
        for spec in &e.specifiers {
            let name = spec
                .exported
                .as_ref()
                .map(|i| i.name.to_string())
                .unwrap_or_else(|| spec.local.name.to_string());
            specifiers.push(hir::Export::Named { name });
        }
        if specifiers.is_empty() {
            vec![]
        } else {
            vec![hir::ModuleItem::Stmt(hir::Stmt::ExportNamed {
                specifiers,
            })]
        }
    }
}

pub fn convert_module_item(stmt: &Statement) -> Vec<hir::ModuleItem> {
    // Handle class expression (oxc parses class declarations as VariableDeclaration with ClassExpression init)
    if let Statement::VariableDeclaration(v) = stmt {
        if let Some(decl) = v.declarations.first() {
            if let BindingPattern::BindingIdentifier(_id) = &decl.id {
                if let Some(init) = &decl.init {
                    if matches!(init, Expression::ClassExpression(_)) {
                        if let Expression::ClassExpression(c) = init {
                            return vec![hir::ModuleItem::Decl(class_to_hir(c))];
                        }
                    }
                }
            }
        }
    }
    match stmt {
        Statement::ClassDeclaration(c) => vec![hir::ModuleItem::Decl(class_to_hir(c))],
        Statement::FunctionDeclaration(f) => vec![hir::ModuleItem::Decl(func_to_decl(f))],
        Statement::VariableDeclaration(v) => var_to_decl(v)
            .into_iter()
            .map(hir::ModuleItem::Decl)
            .collect(),
        Statement::ImportDeclaration(i) => vec![import_to_hir(i)],
        Statement::TSInterfaceDeclaration(i) => {
            vec![hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
                name: i.id.name.to_string(),
                generics: vec![],
                type_: hir::Type::Object { members: vec![] },
            }))]
        }
        Statement::ExportDefaultDeclaration(e) => {
            match &e.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    // Convert to Decl::Function with the function's id as name
                    let name = f.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
                    vec![hir::ModuleItem::Decl(hir::Decl::Function(hir::FunctionDecl {
                        name,
                        generics: vec![],
                        params: vec![],
                        return_type: None,
                        body: None,
                        is_async: f.r#async,
                        is_generator: f.generator,
                        decorators: vec![],
                        throws: false,
                        error_type: None,
                    }))]
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    vec![hir::ModuleItem::Decl(class_to_hir(c))]
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(i) => {
                    vec![hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
                        name: i.id.name.to_string(),
                        generics: vec![],
                        type_: hir::Type::Object { members: vec![] },
                    }))]
                }
                // All other expression variants (inherited from Expression via @inherit)
                // These include NumericLiteral, StringLiteral, BooleanLiteral, Identifier, etc.
                ExportDefaultDeclarationKind::NumericLiteral(n) => {
                    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault {
                        expr: hir::Expr::Number(n.value),
                    })]
                }
                ExportDefaultDeclarationKind::StringLiteral(s) => {
                    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault {
                        expr: hir::Expr::String(s.value.to_string()),
                    })]
                }
                ExportDefaultDeclarationKind::BooleanLiteral(b) => {
                    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault {
                        expr: hir::Expr::Boolean(b.value),
                    })]
                }
                ExportDefaultDeclarationKind::NullLiteral(_) => {
                    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault {
                        expr: hir::Expr::Null,
                    })]
                }
                ExportDefaultDeclarationKind::Identifier(id) => {
                    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault {
                        expr: hir::Expr::Ident { name: id.name.to_string() },
                    })]
                }
                ExportDefaultDeclarationKind::ArrowFunctionExpression(a) => {
                    if let Some(expr) = conv_arrow(a) {
                        vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault { expr })]
                    } else {
                        vec![hir::ModuleItem::Stmt(hir::Stmt::Empty)]
                    }
                }
                // For other expression types, convert as expressions if possible
                _ => {
                    // Try to use convert_expr on the declaration directly
                    // Since ExportDefaultDeclarationKind inherits Expression variants,
                    // we can use expression conversion
                    let expr = export_default_kind_to_expr(&e.declaration)
                        .unwrap_or(hir::Expr::Undefined);
                    vec![hir::ModuleItem::Stmt(hir::Stmt::ExportDefault { expr })]
                }
            }
        }
        Statement::ExportNamedDeclaration(e) => convert_export_named(e),
        _ => vec![hir::ModuleItem::Stmt(stmt_to_hir_stmt(stmt))],
    }
}
