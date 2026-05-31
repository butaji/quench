//! Expression conversion
// allow:complexity

use crate::transpile::hir;
use oxc_ast::ast::*;

pub fn convert_expr(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        Expression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        Expression::NullLiteral(_) => Some(hir::Expr::Null),
        Expression::Identifier(id) => Some(hir::Expr::Ident {
            name: id.name.to_string(),
        }),
        Expression::ArrayExpression(a) => Some(hir::Expr::Array {
            elems: arr_elems(&a),
        }),
        Expression::ObjectExpression(o) => conv_object(o),
        Expression::TemplateLiteral(t) => conv_template(t),
        Expression::BinaryExpression(b) => conv_bin(b),
        Expression::LogicalExpression(l) => conv_log(l),
        Expression::ConditionalExpression(c) => conv_cond(c),
        Expression::AssignmentExpression(a) => conv_assign(a),
        Expression::ArrowFunctionExpression(a) => conv_arrow(a),
        Expression::CallExpression(c) => conv_call(c),
        Expression::NewExpression(n) => conv_new(n),
        Expression::UpdateExpression(u) => conv_update(u),
        Expression::UnaryExpression(u) => conv_unary(u),
        Expression::ParenthesizedExpression(p) => convert_expr(&p.expression),
        Expression::ComputedMemberExpression(m) => conv_computed_member(m),
        Expression::StaticMemberExpression(m) => conv_static_member(m),
        Expression::JSXElement(elem) => Some(hir::Expr::JSX(
            crate::transpile::parser::jsx::convert_jsx_element(elem)
        )),
        Expression::JSXFragment(frag) => Some(hir::Expr::JSX(
            crate::transpile::parser::jsx::convert_jsx_fragment(frag)
        )),
        _ => None,
    }
}

pub fn conv_template(t: &TemplateLiteral) -> Option<hir::Expr> {
    let mut parts = vec![];
    let mut exprs = vec![];

    for quasi in &t.quasis {
        parts.push(hir::TemplatePart::String(quasi.value.raw.to_string()));
    }
    for expr in &t.expressions {
        exprs.push(convert_expr(expr)?);
    }

    Some(hir::Expr::Template { parts, exprs })
}

pub fn conv_object(o: &ObjectExpression) -> Option<hir::Expr> {
    let members: Vec<hir::ObjectMemberExpr> = o
        .properties
        .iter()
        .filter_map(|prop| match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                let key = match &p.key {
                    PropertyKey::StaticIdentifier(i) => hir::PropKey::Str(i.name.to_string()),
                    PropertyKey::StringLiteral(s) => hir::PropKey::Str(s.value.to_string()),
                    PropertyKey::NumericLiteral(n) => hir::PropKey::Num(n.value),
                    _ => return None,
                };
                let value = convert_expr(&p.value)?;
                Some(hir::ObjectMemberExpr {
                    prop: hir::ObjectProp::Init {
                        key,
                        value,
                        computed: p.computed,
                    },
                })
            }
            ObjectPropertyKind::SpreadProperty(_) => None,
        })
        .collect();
    Some(hir::Expr::Object { members })
}

pub fn arr_elems(a: &ArrayExpression) -> Vec<Option<hir::Expr>> {
    a.elements
        .iter()
        .map(|e| e.as_expression().and_then(convert_expr))
        .collect()
}

fn conv_bin(b: &BinaryExpression) -> Option<hir::Expr> {
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

fn conv_log(l: &LogicalExpression) -> Option<hir::Expr> {
    let op = match l.operator {
        LogicalOperator::And => hir::LogicalOp::And,
        LogicalOperator::Or => hir::LogicalOp::Or,
        LogicalOperator::Coalesce => hir::LogicalOp::NullishCoalescing,
    };
    Some(hir::Expr::Logical {
        op,
        left: Box::new(convert_expr(&l.left)?),
        right: Box::new(convert_expr(&l.right)?),
    })
}

fn conv_cond(c: &ConditionalExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Cond {
        test: Box::new(convert_expr(&c.test)?),
        consequent: Box::new(convert_expr(&c.consequent)?),
        alternate: Box::new(convert_expr(&c.alternate)?),
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
        _ => return None, // Logical operators need special handling
    };
    Some(hir::Expr::Assign {
        op,
        left: Box::new(conv_assignment_target(&a.left)?),
        right: Box::new(convert_expr(&a.right)?),
    })
}

fn conv_assignment_target(target: &AssignmentTarget) -> Option<hir::Expr> {
    use AssignmentTarget as At;
    match target {
        At::AssignmentTargetIdentifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        At::StaticMemberExpression(m) => Some(hir::Expr::StaticMember {
            obj: Box::new(convert_expr(&m.object)?),
            property: m.property.name.to_string(),
        }),
        At::ComputedMemberExpression(m) => Some(hir::Expr::Member {
            obj: Box::new(convert_expr(&m.object)?),
            property: Box::new(convert_expr(&m.expression)?),
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
            obj: Box::new(convert_expr(&m.object)?),
            property: m.property.name.to_string(),
        }),
        Sat::ComputedMemberExpression(m) => Some(hir::Expr::Member {
            obj: Box::new(convert_expr(&m.object)?),
            property: Box::new(convert_expr(&m.expression)?),
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
                None
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

/// Convert a statement for use in arrow function block bodies.
/// Handles the common cases without requiring stmt_to_hir_stmt (avoids circular imports).
fn arrow_stmt_to_hir(s: &Statement) -> Option<hir::Stmt> {
    match s {
        Statement::ExpressionStatement(e) => Some(hir::Stmt::Expr {
            expr: convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined),
        }),
        Statement::ReturnStatement(r) => Some(hir::Stmt::Return {
            arg: r.argument.as_ref().and_then(|a| convert_expr(a)),
        }),
        Statement::VariableDeclaration(v) => {
            // Convert to a Block with assignments (same logic as stmt_to_hir_stmt)
            let mut stmts = vec![];
            for decl in &v.declarations {
                let name = match &decl.id {
                    BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                    _ => continue,
                };
                let init = decl.init.as_ref().and_then(convert_expr);
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
                            let init = d.init.as_ref().and_then(convert_expr);
                            Some((name, init))
                        })
                        .collect();
                    Some(hir::ForInit::Variable(kind, vars))
                }
                Some(_) => {
                    // ForStatementInit has inherited Expression variants
                    // Use match_variant! or similar to handle - but for simplicity,
                    // we just convert expressions we know
                    None // For complex cases, fall back to no init
                }
                None => None,
            };
            Some(hir::Stmt::For {
                init,
                test: stmt.test.as_ref().and_then(|t| convert_expr(t)),
                update: stmt.update.as_ref().and_then(|u| convert_expr(u)),
                body: Box::new(
                    arrow_stmt_to_hir(&stmt.body).unwrap_or(hir::Stmt::Empty)
                ),
            })
        }
        Statement::BreakStatement(_) => Some(hir::Stmt::Break { label: None }),
        Statement::ContinueStatement(_) => Some(hir::Stmt::Continue { label: None }),
        Statement::EmptyStatement(_) => Some(hir::Stmt::Empty),
        _ => None,
    }
}

fn conv_call(c: &CallExpression) -> Option<hir::Expr> {
    let callee = Box::new(convert_expr(&c.callee)?);
    let args: Vec<hir::Expr> = c
        .arguments
        .iter()
        .filter_map(|a| a.as_expression().and_then(convert_expr))
        .collect();
    Some(hir::Expr::Call {
        callee,
        arguments: args,
    })
}

fn conv_new(n: &NewExpression) -> Option<hir::Expr> {
    Some(hir::Expr::New {
        callee: Box::new(convert_expr(&n.callee)?),
        arguments: n
            .arguments
            .iter()
            .filter_map(|a| a.as_expression().and_then(convert_expr))
            .collect(),
    })
}

fn conv_computed_member(m: &ComputedMemberExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Member {
        obj: Box::new(convert_expr(&m.object)?),
        property: Box::new(convert_expr(&m.expression)?),
        computed: true,
    })
}

fn conv_static_member(m: &StaticMemberExpression) -> Option<hir::Expr> {
    Some(hir::Expr::StaticMember {
        obj: Box::new(convert_expr(&m.object)?),
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
        arg: Box::new(convert_expr(&u.argument)?),
        prefix: true,
    })
}
