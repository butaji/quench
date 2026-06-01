//! Expression conversion
// allow:complexity

use crate::transpile::hir;
use oxc_ast::ast::*;
use anyhow::{anyhow, bail};

pub fn convert_expr(expr: &Expression) -> Result<hir::Expr, anyhow::Error> {
    match expr {
        Expression::BooleanLiteral(b) => Ok(hir::Expr::Boolean(b.value)),
        Expression::NumericLiteral(n) => Ok(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Ok(hir::Expr::String(s.value.to_string())),
        Expression::NullLiteral(_) => Ok(hir::Expr::Null),
        Expression::Identifier(id) => Ok(hir::Expr::Ident {
            name: id.name.to_string(),
        }),
        Expression::BigIntLiteral(b) => Ok(hir::Expr::BigInt(b.raw.as_ref().map(|s| s.to_string()).unwrap_or_else(|| b.value.to_string()).parse().unwrap_or(0))),
        Expression::RegExpLiteral(r) => Ok(hir::Expr::RegExp {
            pattern: r.regex.pattern.text.to_string(),
            flags: r.regex.flags.to_string(),
        }),
        Expression::ArrayExpression(a) => Ok(hir::Expr::Array {
            elems: arr_elems(&a),
        }),
        Expression::ObjectExpression(o) => conv_object(o),
        Expression::TemplateLiteral(t) => conv_template(t),
        Expression::BinaryExpression(b) => conv_bin(b).ok_or_else(|| anyhow!("Unsupported binary operator")),
        Expression::LogicalExpression(l) => conv_log(l).ok_or_else(|| anyhow!("Unsupported logical expression")),
        Expression::ConditionalExpression(c) => conv_cond(c).ok_or_else(|| anyhow!("Unsupported conditional expression")),
        Expression::AssignmentExpression(a) => conv_assign(a).ok_or_else(|| anyhow!("Unsupported assignment expression")),
        Expression::ArrowFunctionExpression(a) => conv_arrow(a).ok_or_else(|| anyhow!("Unsupported arrow function")),
        Expression::CallExpression(c) => conv_call(c).ok_or_else(|| anyhow!("Unsupported call expression")),
        Expression::NewExpression(n) => conv_new(n).ok_or_else(|| anyhow!("Unsupported new expression")),
        Expression::UpdateExpression(u) => conv_update(u).ok_or_else(|| anyhow!("Unsupported update expression")),
        Expression::UnaryExpression(u) => conv_unary(u).ok_or_else(|| anyhow!("Unsupported unary expression")),
        Expression::ParenthesizedExpression(p) => convert_expr(&p.expression),
        Expression::ComputedMemberExpression(m) => conv_computed_member(m).ok_or_else(|| anyhow!("Unsupported computed member")),
        Expression::StaticMemberExpression(m) => conv_static_member(m).ok_or_else(|| anyhow!("Unsupported static member")),
        Expression::JSXElement(elem) => Ok(hir::Expr::JSX(
            crate::transpile::parser::jsx::convert_jsx_element(elem)
        )),
        Expression::JSXFragment(frag) => Ok(hir::Expr::JSX(
            crate::transpile::parser::jsx::convert_jsx_fragment(frag)
        )),
        Expression::AwaitExpression(a) => Ok(hir::Expr::Await {
            arg: Box::new(convert_expr(&a.argument)?),
        }),
        Expression::YieldExpression(y) => Ok(hir::Expr::Yield {
            arg: y.argument.as_ref().map(|a| convert_expr(a)).transpose()?.map(Box::new),
            delegate: y.delegate,
        }),
        Expression::Super(_) => Ok(hir::Expr::Super),
        Expression::ThisExpression(_) => Ok(hir::Expr::This),
        Expression::MetaProperty(m) => {
            let kind = if m.meta.name == "new" {
                hir::MetaPropKind::NewTarget
            } else if m.meta.name == "import" {
                hir::MetaPropKind::ImportMeta
            } else {
                return Err(anyhow!("Unknown meta property"));
            };
            Ok(hir::Expr::MetaProperty { kind })
        }
        Expression::ImportExpression(i) => {
            // Handle import('module')
            // The source is an Expression - for static imports it's a StringLiteral
            let source = match &i.source {
                Expression::StringLiteral(s) => s.value.to_string(),
                _ => String::new(), // Dynamic imports or other cases
            };
            Ok(hir::Expr::Call {
                callee: Box::new(hir::Expr::Ident { name: "__import".to_string() }),
                arguments: vec![hir::Expr::String(source)],
            })
        }
        Expression::ChainExpression(c) => {
            // Chain expressions - handle call and member expressions with optional flag
            match &c.expression {
                ChainElement::CallExpression(call) => {
                    let callee = Box::new(convert_expr(&call.callee)?);
                    let args: Vec<hir::Expr> = call
                        .arguments
                        .iter()
                        .filter_map(|a| a.as_expression().and_then(|e| convert_expr(e).ok()))
                        .collect();
                    Ok(hir::Expr::Call { callee, arguments: args })
                }
                ChainElement::ComputedMemberExpression(m) => {
                    let obj = Box::new(convert_expr(&m.object)?);
                    let property = Box::new(convert_expr(&m.expression)?);
                    Ok(hir::Expr::Member {
                        obj,
                        property,
                        computed: true,
                    })
                }
                ChainElement::StaticMemberExpression(m) => {
                    let obj = Box::new(convert_expr(&m.object)?);
                    Ok(hir::Expr::Member {
                        obj,
                        property: Box::new(hir::Expr::Ident { name: m.property.name.to_string() }),
                        computed: false,
                    })
                }
                ChainElement::PrivateFieldExpression(m) => {
                    // Handle private field access
                    let obj = Box::new(convert_expr(&m.object)?);
                    Ok(hir::Expr::Member {
                        obj,
                        property: Box::new(hir::Expr::Ident { name: m.field.name.to_string() }),
                        computed: false,
                    })
                }
                _ => Err(anyhow!("Unsupported chain expression")),
            }
        }
        Expression::TaggedTemplateExpression(t) => {
            let tag = Box::new(convert_expr(&t.tag)?);
            let template = Box::new(conv_template(&t.quasi)?);
            Ok(hir::Expr::TaggedTemplate { tag, template })
        }
        Expression::PrivateInExpression(p) => {
            // p.left is PrivateIdentifier, convert to Ident
            let left = Box::new(hir::Expr::Ident { name: p.left.name.to_string() });
            let right = Box::new(convert_expr(&p.right)?);
            Ok(hir::Expr::Bin {
                op: hir::BinaryOp::In,
                left,
                right,
            })
        }
        Expression::SequenceExpression(s) => {
            // SequenceExpression has expressions: Vec<Expression>
            // Convert to sequential pairs
            let mut iter = s.expressions.iter();
            let first = iter.next().and_then(|e| convert_expr(e).ok()).unwrap_or(hir::Expr::Undefined);
            let result = iter.fold(first, |acc, expr| {
                let right = convert_expr(expr).unwrap_or(hir::Expr::Undefined);
                hir::Expr::Seq { left: Box::new(acc), right: Box::new(right) }
            });
            Ok(result)
        }
        Expression::ClassExpression(c) => Ok(hir::Expr::Class {
            id: c.id.as_ref().map(|i| i.name.to_string()),
            super_class: c.super_class.as_ref().map(|s| Box::new(convert_expr(s).unwrap_or(hir::Expr::Undefined))),
            members: vec![],
        }),
        Expression::FunctionExpression(f) => {
            let decl = crate::transpile::parser::stmt::func_to_decl(f);
            if let hir::Decl::Function(func_decl) = decl {
                Ok(hir::Expr::Function(func_decl))
            } else {
                Err(anyhow!("Failed to convert function expression"))
            }
        }
                // TypeScript expression variants - handled above

        // TypeScript expression variants - unwrap inner expression
        Expression::TSAsExpression(e) => convert_expr(&e.expression),
        Expression::TSSatisfiesExpression(e) => convert_expr(&e.expression),
        Expression::TSTypeAssertion(e) => convert_expr(&e.expression),
        Expression::TSNonNullExpression(e) => convert_expr(&e.expression),
        Expression::TSInstantiationExpression(e) => convert_expr(&e.expression),
        // V8 intrinsic expressions (e.g., %GetOptimizationStatus)
        Expression::V8IntrinsicExpression(_) => Err(anyhow!("V8 intrinsics not supported")),
        // Private field expressions - handle via member expression path
        Expression::PrivateFieldExpression(_) => Err(anyhow!("Private field expressions not supported")),
    }
}

pub fn conv_template(t: &TemplateLiteral) -> Result<hir::Expr, anyhow::Error> {
    let mut parts = vec![];
    let mut exprs = vec![];

    for quasi in &t.quasis {
        parts.push(hir::TemplatePart::String(quasi.value.raw.to_string()));
    }
    for expr in &t.expressions {
        exprs.push(convert_expr(expr)?);
    }

    Ok(hir::Expr::Template { parts, exprs })
}

pub fn conv_object(o: &ObjectExpression) -> Result<hir::Expr, anyhow::Error> {
    let members: Vec<hir::ObjectMemberExpr> = o
        .properties
        .iter()
        .filter_map(|prop| match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                let key = match &p.key {
                    PropertyKey::StaticIdentifier(i) => hir::PropKey::Str(i.name.to_string()),
                    PropertyKey::StringLiteral(s) => hir::PropKey::Str(s.value.to_string()),
                    PropertyKey::NumericLiteral(n) => hir::PropKey::Num(n.value),
                    // Computed properties would be handled via PropertyKey::Identifier or other Expression variants
                    // For now, skip computed properties that aren't simple identifiers
                    _ if p.computed => return None,
                    _ => return None,
                };
                let value = convert_expr(&p.value).ok()?;
                Some(hir::ObjectMemberExpr {
                    prop: hir::ObjectProp::Init {
                        key,
                        value,
                        computed: p.computed,
                    },
                })
            }
            ObjectPropertyKind::SpreadProperty(s) => {
                let arg = convert_expr(&s.argument).ok()?;
                Some(hir::ObjectMemberExpr {
                    prop: hir::ObjectProp::Spread { arg },
                })
            }
        })
        .collect();
    Ok(hir::Expr::Object { members })
}

pub fn arr_elems(a: &ArrayExpression) -> Vec<Option<hir::Expr>> {
    a.elements
        .iter()
        .map(|e| e.as_expression().and_then(|e| convert_expr(e).ok()))
        .collect()
}

fn conv_bin(b: &BinaryExpression) -> Option<hir::Expr> {
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

fn conv_log(l: &LogicalExpression) -> Option<hir::Expr> {
    let op = match l.operator {
        LogicalOperator::And => hir::LogicalOp::And,
        LogicalOperator::Or => hir::LogicalOp::Or,
        LogicalOperator::Coalesce => hir::LogicalOp::NullishCoalescing,
    };
    Some(hir::Expr::Logical {
        op,
        left: Box::new(convert_expr(&l.left).ok()?),
        right: Box::new(convert_expr(&l.right).ok()?),
    })
}

fn conv_cond(c: &ConditionalExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Cond {
        test: Box::new(convert_expr(&c.test).ok()?),
        consequent: Box::new(convert_expr(&c.consequent).ok()?),
        alternate: Box::new(convert_expr(&c.alternate).ok()?),
    })
}

fn conv_assign(a: &AssignmentExpression) -> Option<hir::Expr> {
    use oxc_syntax::operator::AssignmentOperator as OxcAssignOp;
    let op = match a.operator {
        OxcAssignOp::Assign => hir::AssignOp::Assign,
        OxcAssignOp::Addition => hir::AssignOp::AddAssign,
        OxcAssignOp::Subtraction => hir::AssignOp::SubAssign,
        OxcAssignOp::Multiplication => hir::AssignOp::MulAssign,
        OxcAssignOp::Division => hir::AssignOp::DivAssign,
        OxcAssignOp::Remainder => hir::AssignOp::ModAssign,
        OxcAssignOp::Exponential => hir::AssignOp::ExpAssign,
        OxcAssignOp::BitwiseXOR => hir::AssignOp::BitXorAssign,
        OxcAssignOp::BitwiseAnd => hir::AssignOp::BitAndAssign,
        OxcAssignOp::BitwiseOR => hir::AssignOp::BitOrAssign,
        OxcAssignOp::ShiftLeft => hir::AssignOp::ShlAssign,
        OxcAssignOp::ShiftRight => hir::AssignOp::ShrAssign,
        OxcAssignOp::ShiftRightZeroFill => hir::AssignOp::UShrAssign,
        OxcAssignOp::LogicalAnd => {
            // x &&= y means x = x && y
            let left = Box::new(conv_assignment_target(&a.left)?);
            let right = Box::new(convert_expr(&a.right).ok()?);
            return Some(hir::Expr::Assign {
                op: hir::AssignOp::Assign,
                left: left.clone(),
                right: Box::new(hir::Expr::Logical {
                    op: hir::LogicalOp::And,
                    left,
                    right,
                }),
            });
        }
        OxcAssignOp::LogicalOr => {
            // x ||= y means x = x || y
            let left = Box::new(conv_assignment_target(&a.left)?);
            let right = Box::new(convert_expr(&a.right).ok()?);
            return Some(hir::Expr::Assign {
                op: hir::AssignOp::Assign,
                left: left.clone(),
                right: Box::new(hir::Expr::Logical {
                    op: hir::LogicalOp::Or,
                    left,
                    right,
                }),
            });
        }
        OxcAssignOp::LogicalOr | OxcAssignOp::LogicalNullish => {
            // x ??= y means x = x ?? y
            let left = Box::new(conv_assignment_target(&a.left)?);
            let right = Box::new(convert_expr(&a.right).ok()?);
            return Some(hir::Expr::Assign {
                op: hir::AssignOp::Assign,
                left: left.clone(),
                right: Box::new(hir::Expr::Logical {
                    op: hir::LogicalOp::NullishCoalescing,
                    left,
                    right,
                }),
            });
        }
        _ => return None,
    };
    Some(hir::Expr::Assign {
        op,
        left: Box::new(conv_assignment_target(&a.left)?),
        right: Box::new(convert_expr(&a.right).ok()?),
    })
}

pub fn conv_assignment_target(target: &AssignmentTarget) -> Option<hir::Expr> {
    use AssignmentTarget as At;
    match target {
        At::AssignmentTargetIdentifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        At::StaticMemberExpression(m) => Some(hir::Expr::StaticMember {
            obj: Box::new(convert_expr(&m.object).ok()?),
            property: m.property.name.to_string(),
        }),
        At::ComputedMemberExpression(m) => Some(hir::Expr::Member {
            obj: Box::new(convert_expr(&m.object).ok()?),
            property: Box::new(convert_expr(&m.expression).ok()?),
            computed: true,
        }),
        _ => None,
    }
}

fn conv_simple_assignment_target(target: &SimpleAssignmentTarget) -> Option<hir::Expr> {
    use SimpleAssignmentTarget as Sat;
    match target {
        Sat::AssignmentTargetIdentifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        Sat::StaticMemberExpression(m) => Some(hir::Expr::StaticMember {
            obj: Box::new(convert_expr(&m.object).ok()?),
            property: m.property.name.to_string(),
        }),
        Sat::ComputedMemberExpression(m) => Some(hir::Expr::Member {
            obj: Box::new(convert_expr(&m.object).ok()?),
            property: Box::new(convert_expr(&m.expression).ok()?),
            computed: true,
        }),
        _ => None,
    }
}

pub fn conv_arrow(a: &ArrowFunctionExpression) -> Option<hir::Expr> {
    let params: Vec<hir::Param> = a
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
                // Handle destructuring patterns in arrow function params
                let pattern = convert_binding_pattern(&p.pattern)?;
                Some(hir::Param {
                    name: String::new(), // Destructuring doesn't have a single name
                    type_: None,
                    default: None,
                    optional: p.optional,
                    pattern: Some(pattern),
                    ownership: hir::Ownership::Owned,
                })
            }
        })
        .collect();
    let body = if a.expression {
        // Arrow function with expression body: () => expr
        if let Some(stmt) = a.body.statements.first() {
            if let Statement::ExpressionStatement(e) = stmt {
                convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined)
            } else {
                hir::Expr::Undefined
            }
        } else {
            hir::Expr::Undefined
        }
    } else {
        // Arrow function with block body: () => { ... }
        // Convert all statements using inline conversion
        let stmts: Vec<hir::Stmt> = a
            .body
            .statements
            .iter()
            .filter_map(|s| arrow_stmt_to_hir(s))
            .collect();
        hir::Expr::Block(stmts)
    };
    Some(hir::Expr::ArrowFunction {
        params,
        body: Box::new(body),
        is_async: a.r#async,
    })
}

/// Convert a binding pattern to a Pat for destructuring
pub fn convert_binding_pattern(pattern: &BindingPattern) -> Option<hir::Pat> {
    match pattern {
        BindingPattern::BindingIdentifier(i) => Some(hir::Pat::Ident {
            name: i.name.to_string(),
            type_: None,
        }),
        BindingPattern::ArrayPattern(a) => {
            let elems: Vec<Option<hir::Pat>> = a
                .elements
                .iter()
                .map(|e| {
                    e.as_ref().and_then(|e| convert_binding_pattern(e))
                })
                .collect();
            let rest = a.rest.as_ref().and_then(|r| {
                convert_binding_pattern(&r.argument)
            }).map(Box::new);
            Some(hir::Pat::Array { elems, rest })
        }
        BindingPattern::ObjectPattern(o) => {
            let props: Vec<hir::ObjectPatProp> = o
                .properties
                .iter()
                .filter_map(|p| {
                    let key = match &p.key {
                        PropertyKey::StaticIdentifier(i) => i.name.to_string(),
                        PropertyKey::StringLiteral(s) => s.value.to_string(),
                        PropertyKey::NumericLiteral(n) => n.value.to_string(),
                        _ => return None,
                    };
                    let value = convert_binding_pattern(&p.value)?;
                    Some(hir::ObjectPatProp::Init { key, value })
                })
                .collect();
            let rest = o.rest.as_ref().and_then(|r| {
                convert_binding_pattern(&r.argument)
            }).map(Box::new);
            Some(hir::Pat::Object { props, rest })
        }
        BindingPattern::AssignmentPattern(_) => {
            // Handle default assignment patterns like `x = 1`
            None
        }
    }
}

/// Convert a statement for use in arrow function block bodies.
/// Handles the common cases without requiring stmt_to_hir_stmt (avoids circular imports).
fn arrow_stmt_to_hir(s: &Statement) -> Option<hir::Stmt> {
    match s {
        Statement::ExpressionStatement(e) => Some(hir::Stmt::Expr {
            expr: convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined),
        }),
        Statement::ReturnStatement(r) => Some(hir::Stmt::Return {
            arg: r.argument.as_ref().and_then(|a| convert_expr(a).ok()),
        }),
        Statement::VariableDeclaration(v) => {
            // Convert to a Block with assignments (same logic as stmt_to_hir_stmt)
            let mut stmts = vec![];
            for decl in &v.declarations {
                let name = match &decl.id {
                    BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                    _ => continue,
                };
                let init = decl.init.as_ref().and_then(|e| convert_expr(e).ok());
                let assign = hir::Expr::Assign {
                    op: hir::AssignOp::Assign,
                    left: Box::new(hir::Expr::Ident { name: name.clone() }),
                    right: Box::new(init.unwrap_or(hir::Expr::Undefined)),
                };
                stmts.push(hir::Stmt::Expr { expr: assign });
            }
            Some(hir::Stmt::Block(stmts))
        }
        Statement::BlockStatement(b) => {
            let stmts: Vec<hir::Stmt> = b
                .body
                .iter()
                .filter_map(arrow_stmt_to_hir)
                .collect();
            Some(hir::Stmt::Block(stmts))
        }
        Statement::IfStatement(stmt) => Some(hir::Stmt::If {
            test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
            consequent: Box::new(
                arrow_stmt_to_hir(&stmt.consequent).unwrap_or(hir::Stmt::Empty)
            ),
            alternate: stmt
                .alternate
                .as_ref()
                .map(|a| Box::new(arrow_stmt_to_hir(a).unwrap_or(hir::Stmt::Empty))),
        }),
        Statement::WhileStatement(stmt) => Some(hir::Stmt::While {
            test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
            body: Box::new(
                arrow_stmt_to_hir(&stmt.body).unwrap_or(hir::Stmt::Empty)
            ),
        }),
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
                                BindingPattern::BindingIdentifier(i) => Some(i.name.to_string()),
                                _ => None,
                            }?;
                            let init = d.init.as_ref().and_then(|e| convert_expr(e).ok());
                            Some((name, init))
                        })
                        .collect();
                    Some(hir::ForInit::Variable(kind, vars))
                }
                _ => {
                    // Handle expression-based for init (e.g., for (i = 0; ...))
                    // Use as_expression() to get the expression if it is one
                    stmt.init.as_ref().and_then(|i| {
                        i.as_expression().and_then(|e| {
                            convert_expr(e).ok().map(|e| hir::ForInit::Expr(Box::new(e)))
                        })
                    })
                }
            };
            Some(hir::Stmt::For {
                init,
                test: stmt.test.as_ref().and_then(|t| convert_expr(t).ok()),
                update: stmt.update.as_ref().and_then(|u| convert_expr(u).ok()),
                body: Box::new(
                    arrow_stmt_to_hir(&stmt.body).unwrap_or(hir::Stmt::Empty)
                ),
            })
        }
        Statement::BreakStatement(_) => Some(hir::Stmt::Break { label: None }),
        Statement::ContinueStatement(_) => Some(hir::Stmt::Continue { label: None }),
        Statement::EmptyStatement(_) => Some(hir::Stmt::Empty),
        // Statements not supported in arrow function bodies or intentionally skipped:
        Statement::WithStatement(_) => None,
        Statement::DoWhileStatement(_) => None,
        Statement::SwitchStatement(_) => None,
        Statement::TryStatement(_) => None,
        Statement::ThrowStatement(_) => None,
        Statement::LabeledStatement(_) => None,
        Statement::ForInStatement(_) => None,
        Statement::ForOfStatement(_) => None,
        Statement::DebuggerStatement(_) => None,
        Statement::ClassDeclaration(_) => None,
        Statement::FunctionDeclaration(_) => None,
        Statement::ImportDeclaration(_) => None,
        Statement::ExportNamedDeclaration(_) => None,
        Statement::ExportDefaultDeclaration(_) => None,
        Statement::ExportAllDeclaration(_) => None,
        Statement::TSEnumDeclaration(_) => None,
        Statement::TSTypeAliasDeclaration(_) => None,
        Statement::TSExportAssignment(_) => None,
        Statement::TSGlobalDeclaration(_) => None,
        Statement::TSNamespaceExportDeclaration(_) => None,
        Statement::TSInterfaceDeclaration(_) => None,
        Statement::TSModuleDeclaration(_) => None,
        Statement::TSImportEqualsDeclaration(_) => None,
        
        // NOTE: No catch-all - new Statement variants will cause compile error.
    }
}

fn conv_call(c: &CallExpression) -> Option<hir::Expr> {
    let callee = Box::new(convert_expr(&c.callee).ok()?);
    let args: Vec<hir::Expr> = c
        .arguments
        .iter()
        .filter_map(|a| a.as_expression().and_then(|e| convert_expr(e).ok()))
        .collect();
    Some(hir::Expr::Call {
        callee,
        arguments: args,
    })
}

fn conv_new(n: &NewExpression) -> Option<hir::Expr> {
    Some(hir::Expr::New {
        callee: Box::new(convert_expr(&n.callee).ok()?),
        arguments: n
            .arguments
            .iter()
            .filter_map(|a| a.as_expression().and_then(|e| convert_expr(e).ok()))
            .collect(),
    })
}

fn conv_computed_member(m: &ComputedMemberExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Member {
        obj: Box::new(convert_expr(&m.object).ok()?),
        property: Box::new(convert_expr(&m.expression).ok()?),
        computed: true,
    })
}

fn conv_static_member(m: &StaticMemberExpression) -> Option<hir::Expr> {
    Some(hir::Expr::StaticMember {
        obj: Box::new(convert_expr(&m.object).ok()?),
        property: m.property.name.to_string(),
    })
}

fn conv_update(u: &UpdateExpression) -> Option<hir::Expr> {
    let op = match u.operator {
        UpdateOperator::Increment => hir::UpdateOp::PlusPlus,
        UpdateOperator::Decrement => hir::UpdateOp::MinusMinus,
    };
    Some(hir::Expr::Update {
        op,
        arg: Box::new(conv_simple_assignment_target(&u.argument)?),
        prefix: u.prefix,
    })
}

fn conv_unary(u: &UnaryExpression) -> Option<hir::Expr> {
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
