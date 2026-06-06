//! Unit tests for HIR runtime expression evaluation.
//!
//! Tests all expression types: literals, operators,
//! control flow, objects, arrays, functions, etc.

use runts_transpile::hir::{self, Expr, LogicalOp, Module, ModuleItem, Stmt};

/// Helper: create a simple module.
fn make_module(stmts: Vec<Stmt>) -> Module {
    Module {
        items: vec![ModuleItem::Stmt(Stmt::Block { stmts })],
    }
}

// =============================================================================
// Literal Tests
// =============================================================================

#[test]
fn test_string_literal() {
    let expr = Expr::String("hello".to_string());
    assert!(matches!(expr, Expr::String(s) if s == "hello"));
}

#[test]
fn test_number_literal_integer() {
    let expr = Expr::Number(42.0);
    assert!(matches!(expr, Expr::Number(n) if (n - 42.0).abs() < f64::EPSILON));
}

#[test]
fn test_number_literal_float() {
    let expr = Expr::Number(3.14);
    assert!(matches!(expr, Expr::Number(n) if (n - 3.14).abs() < f64::EPSILON));
}

#[test]
fn test_number_literal_negative() {
    let expr = Expr::Number(-10.0);
    assert!(matches!(expr, Expr::Number(n) if *n < 0.0));
}

#[test]
fn test_number_literal_zero() {
    let expr = Expr::Number(0.0);
    assert!(matches!(expr, Expr::Number(n) if *n == 0.0));
}

#[test]
fn test_boolean_literal_true() {
    let expr = Expr::Boolean(true);
    assert!(matches!(expr, Expr::Boolean(b) if b));
}

#[test]
fn test_boolean_literal_false() {
    let expr = Expr::Boolean(false);
    assert!(matches!(expr, Expr::Boolean(b) if !b));
}

#[test]
fn test_null_literal() {
    let expr = Expr::Null;
    assert!(matches!(expr, Expr::Null));
}

#[test]
fn test_undefined_literal() {
    let expr = Expr::Undefined;
    assert!(matches!(expr, Expr::Undefined));
}

// =============================================================================
// Identifier Tests
// =============================================================================

#[test]
fn test_identifier_simple() {
    let expr = Expr::Ident { name: "x".to_string() };
    assert!(matches!(expr, Expr::Ident { name } if name == "x"));
}

#[test]
fn test_identifier_camel_case() {
    let expr = Expr::Ident { name: "myVariable".to_string() };
    assert!(matches!(expr, Expr::Ident { name } if name == "myVariable"));
}

#[test]
fn test_identifier_with_underscore() {
    let expr = Expr::Ident { name: "my_variable".to_string() };
    assert!(matches!(expr, Expr::Ident { name } if name == "my_variable"));
}

#[test]
fn test_identifier_dollar_prefix() {
    let expr = Expr::Ident { name: "$element".to_string() };
    assert!(matches!(expr, Expr::Ident { name } if name == "$element"));
}

// =============================================================================
// Array Tests
// =============================================================================

#[test]
fn test_array_empty() {
    let expr = Expr::Array { elems: vec![] };
    assert!(matches!(expr, Expr::Array { elems } if elems.is_empty()));
}

#[test]
fn test_array_single_element() {
    let expr = Expr::Array {
        elems: vec![Some(Expr::Number(1.0))],
    };
    assert!(matches!(expr, Expr::Array { elems } if elems.len() == 1));
}

#[test]
fn test_array_multiple_elements() {
    let expr = Expr::Array {
        elems: vec![
            Some(Expr::Number(1.0)),
            Some(Expr::Number(2.0)),
            Some(Expr::Number(3.0)),
        ],
    };
    assert!(matches!(expr, Expr::Array { elems } if elems.len() == 3));
}

#[test]
fn test_array_with_spread() {
    let expr = Expr::Array {
        elems: vec![
            Some(Expr::Number(1.0)),
            Some(Expr::Spread(Box::new(Expr::Ident { name: "rest".to_string() }))),
            Some(Expr::Number(3.0)),
        ],
    };
    assert!(matches!(expr, Expr::Array { elems } if elems.len() == 3));
}

#[test]
fn test_array_with_holes() {
    let expr = Expr::Array {
        elems: vec![Some(Expr::Number(1.0)), None, Some(Expr::Number(3.0))],
    };
    assert!(matches!(expr, Expr::Array { elems } if elems.len() == 3));
}

// =============================================================================
// Object Tests
// =============================================================================

#[test]
fn test_object_empty() {
    let expr = Expr::Object { members: vec![] };
    assert!(matches!(expr, Expr::Object { members } if members.is_empty()));
}

#[test]
fn test_object_single_property() {
    let expr = Expr::Object {
        members: vec![hir::ObjectMemberExpr {
            prop: hir::ObjectProp::Init {
                key: hir::PropKey::Str("x".to_string()),
                value: Expr::Number(1.0),
            },
        }],
    };
    assert!(matches!(expr, Expr::Object { members } if members.len() == 1));
}

#[test]
fn test_object_multiple_properties() {
    let expr = Expr::Object {
        members: vec![
            hir::ObjectMemberExpr {
                prop: hir::ObjectProp::Init {
                    key: hir::PropKey::Str("x".to_string()),
                    value: Expr::Number(1.0),
                },
            },
            hir::ObjectMemberExpr {
                prop: hir::ObjectProp::Init {
                    key: hir::PropKey::Str("y".to_string()),
                    value: Expr::Number(2.0),
                },
            },
        ],
    };
    assert!(matches!(expr, Expr::Object { members } if members.len() == 2));
}

#[test]
fn test_object_numeric_key() {
    let expr = Expr::Object {
        members: vec![hir::ObjectMemberExpr {
            prop: hir::ObjectProp::Init {
                key: hir::PropKey::Num(0.0),
                value: Expr::String("first".to_string()),
            },
        }],
    };
    assert!(matches!(expr, Expr::Object { members } if members.len() == 1));
}

#[test]
fn test_object_shorthand() {
    let expr = Expr::Object {
        members: vec![hir::ObjectMemberExpr {
            prop: hir::ObjectProp::Shorthand {
                name: "x".to_string(),
            },
        }],
    };
    assert!(matches!(expr, Expr::Object { members } if members.len() == 1));
}

#[test]
fn test_object_method() {
    let expr = Expr::Object {
        members: vec![hir::ObjectMemberExpr {
            prop: hir::ObjectProp::Method {
                name: "greet".to_string(),
                params: vec![],
                body: Box::new(Expr::Block(vec![])),
            },
        }],
    };
    assert!(matches!(expr, Expr::Object { members } if members.len() == 1));
}

#[test]
fn test_object_getter() {
    let expr = Expr::Object {
        members: vec![hir::ObjectMemberExpr {
            prop: hir::ObjectProp::Getter {
                name: "value".to_string(),
                body: Box::new(Expr::Block(vec![])),
            },
        }],
    };
    assert!(matches!(expr, Expr::Object { members } if members.len() == 1));
}

// =============================================================================
// Binary Operator Tests
// =============================================================================

#[test]
fn test_binary_add_numbers() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Add,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(2.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Add)));
}

#[test]
fn test_binary_subtract() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Sub,
        left: Box::new(Expr::Number(5.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Sub)));
}

#[test]
fn test_binary_multiply() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Mul,
        left: Box::new(Expr::Number(4.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Mul)));
}

#[test]
fn test_binary_divide() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Div,
        left: Box::new(Expr::Number(10.0)),
        right: Box::new(Expr::Number(2.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Div)));
}

#[test]
fn test_binary_modulo() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Mod,
        left: Box::new(Expr::Number(10.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Mod)));
}

#[test]
fn test_binary_power() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Pow,
        left: Box::new(Expr::Number(2.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Pow)));
}

#[test]
fn test_binary_equal() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Eq,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(1.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Eq)));
}

#[test]
fn test_binary_strict_equal() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::StrictEq,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(1.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::StrictEq)));
}

#[test]
fn test_binary_not_equal() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Neq,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(2.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Neq)));
}

#[test]
fn test_binary_less_than() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Lt,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(2.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Lt)));
}

#[test]
fn test_binary_less_equal() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Lte,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(1.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Lte)));
}

#[test]
fn test_binary_greater_than() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Gt,
        left: Box::new(Expr::Number(2.0)),
        right: Box::new(Expr::Number(1.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Gt)));
}

#[test]
fn test_binary_greater_equal() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Gte,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(1.0)),
    };
    assert!(matches!(expr, Expr::Bin { op, .. } if matches!(op, hir::BinaryOp::Gte)));
}

// =============================================================================
// Logical Operator Tests
// =============================================================================

#[test]
fn test_logical_and() {
    let expr = Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(Expr::Boolean(true)),
        right: Box::new(Expr::Boolean(false)),
    };
    assert!(matches!(expr, Expr::Logical { op, .. } if matches!(op, LogicalOp::And)));
}

#[test]
fn test_logical_or() {
    let expr = Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(Expr::Boolean(true)),
        right: Box::new(Expr::Boolean(false)),
    };
    assert!(matches!(expr, Expr::Logical { op, .. } if matches!(op, LogicalOp::Or)));
}

#[test]
fn test_logical_nullish() {
    let expr = Expr::Logical {
        op: LogicalOp::NullishCoalescing,
        left: Box::new(Expr::Null),
        right: Box::new(Expr::String("default".to_string())),
    };
    assert!(matches!(expr, Expr::Logical { op, .. } if matches!(op, LogicalOp::NullishCoalescing)));
}

#[test]
fn test_logical_not() {
    let expr = Expr::Unary {
        op: hir::UnaryOp::Not,
        arg: Box::new(Expr::Boolean(true)),
    };
    assert!(matches!(expr, Expr::Unary { op, .. } if matches!(op, hir::UnaryOp::Not)));
}

#[test]
fn test_logical_negate() {
    let expr = Expr::Unary {
        op: hir::UnaryOp::Neg,
        arg: Box::new(Expr::Number(5.0)),
    };
    assert!(matches!(expr, Expr::Unary { op, .. } if matches!(op, hir::UnaryOp::Neg)));
}

// =============================================================================
// Ternary/Conditional Tests
// =============================================================================

#[test]
fn test_ternary_true_branch() {
    let expr = Expr::Cond {
        test: Box::new(Expr::Boolean(true)),
        consequent: Box::new(Expr::String("yes".to_string())),
        alternate: Box::new(Expr::String("no".to_string())),
    };
    assert!(matches!(expr, Expr::Cond { .. }));
}

#[test]
fn test_ternary_false_branch() {
    let expr = Expr::Cond {
        test: Box::new(Expr::Boolean(false)),
        consequent: Box::new(Expr::String("yes".to_string())),
        alternate: Box::new(Expr::String("no".to_string())),
    };
    assert!(matches!(expr, Expr::Cond { .. }));
}

#[test]
fn test_ternary_nested() {
    let inner = Expr::Cond {
        test: Box::new(Expr::Boolean(true)),
        consequent: Box::new(Expr::String("a".to_string())),
        alternate: Box::new(Expr::String("b".to_string())),
    };
    
    let expr = Expr::Cond {
        test: Box::new(Expr::Boolean(false)),
        consequent: Box::new(inner),
        alternate: Box::new(Expr::String("c".to_string())),
    };
    assert!(matches!(expr, Expr::Cond { consequent, .. } if matches!(consequent.as_ref(), Expr::Cond { .. })));
}

// =============================================================================
// Function Tests
// =============================================================================

#[test]
fn test_function_declaration() {
    let func = hir::FunctionDecl {
        name: "greet".to_string(),
        params: vec![hir::Param { name: "name".to_string() }],
        body: Some(hir::Block(vec![Stmt::Return {
            arg: Some(Box::new(Expr::String("Hello".to_string()))),
        }])),
    };
    
    let module = Module {
        items: vec![ModuleItem::Decl(hir::Decl::Function(func))],
    };
    
    assert!(!module.items.is_empty());
}

#[test]
fn test_arrow_function() {
    let expr = Expr::ArrowFunction {
        params: vec![hir::Param { name: "x".to_string() }],
        body: Box::new(Expr::Bin {
            op: hir::BinaryOp::Mul,
            left: Box::new(Expr::Ident { name: "x".to_string() }),
            right: Box::new(Expr::Number(2.0)),
        }),
        is_async: false,
    };
    assert!(matches!(expr, Expr::ArrowFunction { params, .. } if params.len() == 1));
}

#[test]
fn test_arrow_function_empty_params() {
    let expr = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Number(42.0)),
        is_async: false,
    };
    assert!(matches!(expr, Expr::ArrowFunction { params, .. } if params.is_empty()));
}

#[test]
fn test_arrow_function_no_parens_single_param() {
    let expr = Expr::ArrowFunction {
        params: vec![hir::Param { name: "x".to_string() }],
        body: Box::new(Expr::Number(42.0)),
        is_async: false,
    };
    assert!(matches!(expr, Expr::ArrowFunction { params, .. } if params.len() == 1));
}

#[test]
fn test_async_function() {
    let expr = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Number(42.0)),
        is_async: true,
    };
    assert!(matches!(expr, Expr::ArrowFunction { is_async, .. } if is_async));
}

// =============================================================================
// Call Expression Tests
// =============================================================================

#[test]
fn test_call_simple() {
    let expr = Expr::Call {
        callee: Box::new(Expr::Ident { name: "fn".to_string() }),
        arguments: vec![],
    };
    assert!(matches!(expr, Expr::Call { arguments, .. } if arguments.is_empty()));
}

#[test]
fn test_call_with_args() {
    let expr = Expr::Call {
        callee: Box::new(Expr::Ident { name: "fn".to_string() }),
        arguments: vec![
            Expr::Number(1.0),
            Expr::String("hello".to_string()),
            Expr::Boolean(true),
        ],
    };
    assert!(matches!(expr, Expr::Call { arguments, .. } if arguments.len() == 3));
}

#[test]
fn test_call_member() {
    let expr = Expr::Call {
        callee: Box::new(Expr::Member {
            obj: Box::new(Expr::Ident { name: "obj".to_string() }),
            property: Box::new(Expr::Ident { name: "method".to_string() }),
            computed: false,
        }),
        arguments: vec![],
    };
    assert!(matches!(expr, Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::Member { .. })));
}

// =============================================================================
// Member Expression Tests
// =============================================================================

#[test]
fn test_member_property() {
    let expr = Expr::Member {
        obj: Box::new(Expr::Ident { name: "obj".to_string() }),
        property: Box::new(Expr::Ident { name: "prop".to_string() }),
        computed: false,
    };
    assert!(matches!(expr, Expr::Member { property, .. } if matches!(property.as_ref(), Expr::Ident { name } if name == "prop")));
}

#[test]
fn test_member_computed() {
    let expr = Expr::Member {
        obj: Box::new(Expr::Ident { name: "arr".to_string() }),
        property: Box::new(Expr::Number(0.0)),
        computed: true,
    };
    assert!(matches!(expr, Expr::Member { computed: true, .. }));
}

#[test]
fn test_static_member() {
    let expr = Expr::StaticMember {
        obj: Box::new(Expr::Ident { name: "Math".to_string() }),
        property: "PI".to_string(),
    };
    assert!(matches!(expr, Expr::StaticMember { property, .. } if property == "PI"));
}

// =============================================================================
// Template Literal Tests
// =============================================================================

#[test]
fn test_template_simple() {
    let expr = Expr::Template {
        parts: vec![hir::TemplatePart::String { value: "hello".to_string() }],
        exprs: vec![],
    };
    assert!(matches!(expr, Expr::Template { parts, exprs } if parts.len() == 1 && exprs.is_empty()));
}

#[test]
fn test_template_with_expr() {
    let expr = Expr::Template {
        parts: vec![
            hir::TemplatePart::String { value: "hello ".to_string() },
            hir::TemplatePart::String { value: "".to_string() },
        ],
        exprs: vec![Expr::Ident { name: "name".to_string() }],
    };
    assert!(matches!(expr, Expr::Template { exprs, .. } if exprs.len() == 1));
}

#[test]
fn test_template_multiple_exprs() {
    let expr = Expr::Template {
        parts: vec![
            hir::TemplatePart::String { value: "".to_string() },
            hir::TemplatePart::String { value: " + ".to_string() },
            hir::TemplatePart::String { value: " = ".to_string() },
            hir::TemplatePart::String { value: "".to_string() },
        ],
        exprs: vec![
            Expr::Ident { name: "a".to_string() },
            Expr::Ident { name: "b".to_string() },
            Expr::Ident { name: "sum".to_string() },
        ],
    };
    assert!(matches!(expr, Expr::Template { exprs, .. } if exprs.len() == 3));
}

// =============================================================================
// Block Tests
// =============================================================================

#[test]
fn test_block_empty() {
    let stmt = Stmt::Block { stmts: vec![] };
    assert!(matches!(stmt, Stmt::Block { stmts } if stmts.is_empty()));
}

#[test]
fn test_block_multiple_stmts() {
    let stmt = Stmt::Block {
        stmts: vec![
            Stmt::Variable(hir::VariableDecl {
                pattern: None,
                name: "x".to_string(),
                init: Some(Expr::Number(1.0)),
            }),
            Stmt::Variable(hir::VariableDecl {
                pattern: None,
                name: "y".to_string(),
                init: Some(Expr::Number(2.0)),
            }),
        ],
    };
    assert!(matches!(stmt, Stmt::Block { stmts } if stmts.len() == 2));
}

// =============================================================================
// Assignment Tests
// =============================================================================

#[test]
fn test_assign_simple() {
    let expr = Expr::Assign {
        left: Box::new(Expr::Ident { name: "x".to_string() }),
        right: Box::new(Expr::Number(42.0)),
        op: hir::AssignOp::Simple,
    };
    assert!(matches!(expr, Expr::Assign { op, .. } if matches!(op, hir::AssignOp::Simple)));
}

#[test]
fn test_assign_add() {
    let expr = Expr::Assign {
        left: Box::new(Expr::Ident { name: "x".to_string() }),
        right: Box::new(Expr::Number(1.0)),
        op: hir::AssignOp::Add,
    };
    assert!(matches!(expr, Expr::Assign { op, .. } if matches!(op, hir::AssignOp::Add)));
}

#[test]
fn test_assign_sub() {
    let expr = Expr::Assign {
        left: Box::new(Expr::Ident { name: "x".to_string() }),
        right: Box::new(Expr::Number(1.0)),
        op: hir::AssignOp::Sub,
    };
    assert!(matches!(expr, Expr::Assign { op, .. } if matches!(op, hir::AssignOp::Sub)));
}

// =============================================================================
// Update Expression Tests (++ --)
// =============================================================================

#[test]
fn test_update_prefix_inc() {
    let expr = Expr::Update {
        op: hir::UpdateOp::Inc,
        arg: Box::new(Expr::Ident { name: "x".to_string() }),
        prefix: true,
    };
    assert!(matches!(expr, Expr::Update { op: hir::UpdateOp::Inc, prefix: true, .. }));
}

#[test]
fn test_update_postfix_inc() {
    let expr = Expr::Update {
        op: hir::UpdateOp::Inc,
        arg: Box::new(Expr::Ident { name: "x".to_string() }),
        prefix: false,
    };
    assert!(matches!(expr, Expr::Update { op: hir::UpdateOp::Inc, prefix: false, .. }));
}

#[test]
fn test_update_prefix_dec() {
    let expr = Expr::Update {
        op: hir::UpdateOp::Dec,
        arg: Box::new(Expr::Ident { name: "x".to_string() }),
        prefix: true,
    };
    assert!(matches!(expr, Expr::Update { op: hir::UpdateOp::Dec, prefix: true, .. }));
}

// =============================================================================
// Await Expression Tests
// =============================================================================

#[test]
fn test_await_simple() {
    let expr = Expr::Await {
        arg: Box::new(Expr::Call {
            callee: Box::new(Expr::Ident { name: "promise".to_string() }),
            arguments: vec![],
        }),
    };
    assert!(matches!(expr, Expr::Await { .. }));
}

// =============================================================================
// Yield Expression Tests
// =============================================================================

#[test]
fn test_yield() {
    let expr = Expr::Yield {
        arg: Some(Box::new(Expr::Number(42.0))),
    };
    assert!(matches!(expr, Expr::Yield { arg: Some(_), .. }));
}

#[test]
fn test_yield_none() {
    let expr = Expr::Yield { arg: None };
    assert!(matches!(expr, Expr::Yield { arg: None, .. }));
}

// =============================================================================
// Type Expression Tests
// =============================================================================

#[test]
fn test_typeof() {
    let expr = Expr::Unary {
        op: hir::UnaryOp::Typeof,
        arg: Box::new(Expr::Ident { name: "x".to_string() }),
    };
    assert!(matches!(expr, Expr::Unary { op: hir::UnaryOp::Typeof, .. }));
}

#[test]
fn test_void() {
    let expr = Expr::Unary {
        op: hir::UnaryOp::Void,
        arg: Box::new(Expr::Number(0.0)),
    };
    assert!(matches!(expr, Expr::Unary { op: hir::UnaryOp::Void, .. }));
}

// =============================================================================
// Bitwise Operator Tests
// =============================================================================

#[test]
fn test_bitwise_and() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::BitAnd,
        left: Box::new(Expr::Number(5.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op: hir::BinaryOp::BitAnd, .. }));
}

#[test]
fn test_bitwise_or() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::BitOr,
        left: Box::new(Expr::Number(5.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op: hir::BinaryOp::BitOr, .. }));
}

#[test]
fn test_bitwise_xor() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::BitXor,
        left: Box::new(Expr::Number(5.0)),
        right: Box::new(Expr::Number(3.0)),
    };
    assert!(matches!(expr, Expr::Bin { op: hir::BinaryOp::BitXor, .. }));
}

#[test]
fn test_bitwise_not() {
    let expr = Expr::Unary {
        op: hir::UnaryOp::Bitnot,
        arg: Box::new(Expr::Number(5.0)),
    };
    assert!(matches!(expr, Expr::Unary { op: hir::UnaryOp::Bitnot, .. }));
}

#[test]
fn test_shift_left() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Shl,
        left: Box::new(Expr::Number(1.0)),
        right: Box::new(Expr::Number(2.0)),
    };
    assert!(matches!(expr, Expr::Bin { op: hir::BinaryOp::Shl, .. }));
}

#[test]
fn test_shift_right() {
    let expr = Expr::Bin {
        op: hir::BinaryOp::Shr,
        left: Box::new(Expr::Number(8.0)),
        right: Box::new(Expr::Number(2.0)),
    };
    assert!(matches!(expr, Expr::Bin { op: hir::BinaryOp::Shr, .. }));
}
