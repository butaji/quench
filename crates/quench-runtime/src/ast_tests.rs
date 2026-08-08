use super::*;

#[test]
fn test_param_new() {
    let p = Param::new("x");
    assert_eq!(p.name, "x");
    assert!(p.default.is_none());
    assert!(p.pattern.is_none());
    assert!(!p.rest);
}

#[test]
fn test_param_with_default() {
    let p = Param::with_default("y", Expression::Number(42.0));
    assert_eq!(p.name, "y");
    assert!(p.default.is_some());
    assert!(!p.rest);
}

#[test]
fn test_param_rest() {
    let p = Param::rest("args");
    assert_eq!(p.name, "args");
    assert!(p.default.is_none());
    assert!(p.rest);
}

#[test]
fn test_binary_op_precedence_order() {
    assert!(BinaryOp::Pow.precedence() > BinaryOp::Mul.precedence());
    assert!(BinaryOp::Mul.precedence() > BinaryOp::Add.precedence());
    assert!(BinaryOp::Add.precedence() > BinaryOp::Shl.precedence());
    assert!(BinaryOp::Shl.precedence() > BinaryOp::Lt.precedence());
    assert!(BinaryOp::Lt.precedence() > BinaryOp::StrictEq.precedence());
    assert!(BinaryOp::StrictEq.precedence() > BinaryOp::BitAnd.precedence());
    assert!(BinaryOp::BitAnd.precedence() > BinaryOp::BitXor.precedence());
    assert!(BinaryOp::BitXor.precedence() > BinaryOp::BitOr.precedence());
    assert!(BinaryOp::BitOr.precedence() > BinaryOp::And.precedence());
    assert!(BinaryOp::And.precedence() > BinaryOp::Or.precedence());
}

#[test]
fn test_binary_op_all_variants() {
    for op in [
        BinaryOp::And,
        BinaryOp::Or,
        BinaryOp::Eq,
        BinaryOp::Neq,
        BinaryOp::LooseEq,
        BinaryOp::StrictEq,
        BinaryOp::StrictNeq,
        BinaryOp::Lt,
        BinaryOp::Gt,
        BinaryOp::Le,
        BinaryOp::Ge,
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Mod,
        BinaryOp::BitAnd,
        BinaryOp::BitOr,
        BinaryOp::BitXor,
        BinaryOp::Shl,
        BinaryOp::Shr,
        BinaryOp::Ushr,
        BinaryOp::Pow,
        BinaryOp::In,
        BinaryOp::Instanceof,
        BinaryOp::NullishCoalescing,
    ] {
        assert!(op.precedence() > 0);
    }
}

#[test]
fn test_compound_op_to_binary() {
    assert_eq!(CompoundOp::Add.to_binary(), BinaryOp::Add);
    assert_eq!(CompoundOp::Sub.to_binary(), BinaryOp::Sub);
    assert_eq!(CompoundOp::Mul.to_binary(), BinaryOp::Mul);
    assert_eq!(CompoundOp::Div.to_binary(), BinaryOp::Div);
    assert_eq!(CompoundOp::Mod.to_binary(), BinaryOp::Mod);
    assert_eq!(CompoundOp::Pow.to_binary(), BinaryOp::Pow);
    assert_eq!(CompoundOp::BitAnd.to_binary(), BinaryOp::BitAnd);
    assert_eq!(CompoundOp::BitOr.to_binary(), BinaryOp::BitOr);
    assert_eq!(CompoundOp::BitXor.to_binary(), BinaryOp::BitXor);
    assert_eq!(CompoundOp::Shl.to_binary(), BinaryOp::Shl);
    assert_eq!(CompoundOp::Shr.to_binary(), BinaryOp::Shr);
    assert_eq!(CompoundOp::Ushr.to_binary(), BinaryOp::Ushr);
}

#[test]
fn test_compound_op_logical_unreachable() {
    use std::panic;
    assert!(panic::catch_unwind(|| CompoundOp::LogicalOrAssign.to_binary()).is_err());
    assert!(panic::catch_unwind(|| CompoundOp::LogicalAndAssign.to_binary()).is_err());
    assert!(panic::catch_unwind(|| CompoundOp::NullishCoalescingAssign.to_binary()).is_err());
}

#[test]
fn test_unary_op_derives() {
    assert_eq!(UnaryOp::Not, UnaryOp::Not);
    assert_eq!(UnaryOp::Neg, UnaryOp::Neg);
    assert_eq!(UnaryOp::Plus, UnaryOp::Plus);
    assert_eq!(UnaryOp::BitNot, UnaryOp::BitNot);
    assert_eq!(UnaryOp::Typeof, UnaryOp::Typeof);
    assert_eq!(UnaryOp::Void, UnaryOp::Void);
    assert_eq!(UnaryOp::Delete, UnaryOp::Delete);
}

#[test]
fn test_update_op_derives() {
    assert_eq!(UpdateOp::Increment, UpdateOp::Increment);
    assert_eq!(UpdateOp::Decrement, UpdateOp::Decrement);
}

#[test]
fn test_var_kind_derives() {
    assert_eq!(VarKind::Var, VarKind::Var);
    assert_eq!(VarKind::Let, VarKind::Let);
    assert_eq!(VarKind::Const, VarKind::Const);
}

#[test]
fn test_property_key_derives() {
    let k1 = PropertyKey::Ident("a".to_string());
    let k2 = PropertyKey::Ident("a".to_string());
    let k3 = PropertyKey::Ident("b".to_string());
    assert_eq!(k1, k2);
    assert_ne!(k1, k3);
    assert_ne!(
        PropertyKey::Ident("x".to_string()),
        PropertyKey::String("x".to_string())
    );
}

#[test]
fn test_binding_element_derives() {
    let b1 = BindingElement::Identifier("x".to_string());
    let b2 = BindingElement::Identifier("x".to_string());
    let b3 = BindingElement::Identifier("y".to_string());
    assert_eq!(b1, b2);
    assert_ne!(b1, b3);
}

#[test]
fn test_arrow_body_derives() {
    let e = ArrowBody::Expression(Expression::Number(1.0));
    let b = ArrowBody::Block(std::rc::Rc::new(vec![Statement::Empty]));
    assert_eq!(e, ArrowBody::Expression(Expression::Number(1.0)));
    assert_ne!(e, b);
}

#[test]
fn test_span_display() {
    let span = Span { start: 10, end: 20 };
    assert_eq!(format!("{}", span), "10..20");
}

#[test]
fn test_span_default() {
    let span = Span::default();
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 0);
}

#[test]
fn test_program_derives() {
    let p1 = Program::Script(vec![Statement::Empty]);
    let p2 = Program::Script(vec![Statement::Empty]);
    assert_eq!(p1, p2);
}
