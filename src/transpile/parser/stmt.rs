//! Statement conversion
// allow:complexity

use crate::transpile::hir;
use crate::transpile::parser::expr::{
    arr_elems, convert_expr, conv_arrow, conv_object, conv_template,
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
            let name = match &decl.id {
                BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                _ => return None,
            };
            let init = decl.init.as_ref().and_then(convert_expr);
            Some(hir::Decl::Variable(hir::VariableDecl {
                name,
                kind: kind.clone(),
                type_: None,
                init,
                pattern: None,
            }))
        })
        .collect()
}

fn func_to_decl(f: &Function) -> hir::Decl {
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
                None
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
                            let init = d.init.as_ref().and_then(convert_expr);
                            Some((name, init))
                        })
                        .collect();
                    Some(hir::ForInit::Variable(kind, vars))
                }
                // ForStatementInit inherits Expression variants but they have different names
                // For simplicity, only handle VariableDeclaration init
                // Other expression-based init (like `for (i = 0; ...)`) would need special handling
                Some(_) | None => None,
            };
            hir::Stmt::For {
                init,
                test: stmt.test.as_ref().and_then(|t| convert_expr(t)),
                update: stmt.update.as_ref().and_then(|u| convert_expr(u)),
                body: Box::new(stmt_to_hir_stmt(&stmt.body)),
            }
        }
        Statement::ReturnStatement(r) => hir::Stmt::Return {
            arg: r.argument.as_ref().and_then(|a| convert_expr(a)),
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
                let init = decl.init.as_ref().and_then(convert_expr);
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
        _ => hir::Stmt::Empty,
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
                                .and_then(|a| convert_expr(a))
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
                            None
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
        ExportDefaultDeclarationKind::TemplateLiteral(t) => conv_template(t),
        ExportDefaultDeclarationKind::ObjectExpression(o) => conv_object(o),
        ExportDefaultDeclarationKind::ArrayExpression(a) => Some(hir::Expr::Array { elems: arr_elems(a) }),
        ExportDefaultDeclarationKind::ClassExpression(c) => Some(hir::Expr::Class {
            id: c.id.as_ref().map(|i| i.name.to_string()),
            super_class: c.super_class.as_ref().map(|s| Box::new(convert_expr(s).unwrap_or(hir::Expr::Undefined))),
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
            let callee = Box::new(convert_expr(&c.callee)?);
            let args: Vec<hir::Expr> = c
                .arguments
                .iter()
                .filter_map(|a| a.as_expression().and_then(convert_expr))
                .collect();
            Some(hir::Expr::Call { callee, arguments: args })
        }
        ExportDefaultDeclarationKind::StaticMemberExpression(m) => {
            let obj = Box::new(convert_expr(&m.object)?);
            Some(hir::Expr::StaticMember {
                obj,
                property: m.property.name.to_string(),
            })
        }
        ExportDefaultDeclarationKind::ComputedMemberExpression(m) => {
            let obj = Box::new(convert_expr(&m.object)?);
            let property = Box::new(convert_expr(&m.expression)?);
            Some(hir::Expr::Member {
                obj,
                property,
                computed: true,
            })
        }
        ExportDefaultDeclarationKind::BinaryExpression(b) => {
            let left = Box::new(convert_expr(&b.left)?);
            let right = Box::new(convert_expr(&b.right)?);
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
                arg: Box::new(convert_expr(&u.argument)?),
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
                test: Box::new(convert_expr(&c.test)?),
                consequent: Box::new(convert_expr(&c.consequent)?),
                alternate: Box::new(convert_expr(&c.alternate)?),
            })
        }
        ExportDefaultDeclarationKind::ParenthesizedExpression(p) => convert_expr(&p.expression),
        _ => None,
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
        _ => vec![hir::ModuleItem::Stmt(stmt_to_hir_stmt(stmt))],
    }
}
