use crate::ast::{BindingElement, Expression, PropertyKey, Statement, VarKind};
use crate::parser::parse_script;

fn parse_statements(src: &str) -> Vec<Statement> {
    let prog = parse_script(src).unwrap_or_else(|_| panic!("parse failed for: {}", src));
    match prog {
        crate::ast::Program::Script(stmts) => {
            fn flatten(stmts: &[Statement]) -> Vec<Statement> {
                let mut out = Vec::new();
                for s in stmts {
                    match s {
                        Statement::SequenceDecls(inner) => out.extend(flatten(inner)),
                        _ => out.push(s.clone()),
                    }
                }
                out
            }
            flatten(&stmts)
        }
    }
}

fn find_var<'a>(stmts: &'a [Statement], name: &str) -> Option<&'a Statement> {
    stmts.iter().find(|s| {
        if let Statement::VarDeclaration { name: n, .. } = s {
            n == name
        } else {
            false
        }
    })
}

fn var_names(stmts: &[Statement]) -> Vec<String> {
    stmts
        .iter()
        .filter_map(|s| {
            if let Statement::VarDeclaration { name, .. } = s {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect()
}

// ─── Array Destructuring ─────────────────────────────────────────────────

#[test]
fn lower_array_destructure_basic() {
    let stmts = parse_statements("let [a, b] = arr;");
    assert_eq!(var_names(&stmts), &["__arr_src_0", "a", "b"]);

    let src = find_var(&stmts, "__arr_src_0").unwrap();
    if let Statement::VarDeclaration { init: Some(expr), .. } = src {
        assert!(matches!(expr, Expression::Identifier(i) if i == "arr"));
    } else {
        panic!("expected VarDeclaration with init");
    }

    let a_stmt = find_var(&stmts, "a").unwrap();
    if let Statement::VarDeclaration { init: Some(expr), kind: VarKind::Let, .. } = a_stmt {
        if let Expression::Member { property: PropertyKey::String(k), computed: false, .. } = expr {
            assert_eq!(k, "0");
        } else {
            panic!("expected Member with string key '0', got {:?}", expr);
        }
    } else {
        panic!("expected VarDeclaration(let) for 'a' with init");
    }

    let b_stmt = find_var(&stmts, "b").unwrap();
    if let Statement::VarDeclaration { init: Some(expr), kind: VarKind::Let, .. } = b_stmt {
        if let Expression::Member { property: PropertyKey::String(k), computed: false, .. } = expr {
            assert_eq!(k, "1");
        } else {
            panic!("expected Member with string key '1'");
        }
    } else {
        panic!("expected VarDeclaration for 'b'");
    }
}

#[test]
fn lower_array_destructure_with_hole() {
    let stmts = parse_statements("let [, b] = arr;");
    assert_eq!(var_names(&stmts), &["__arr_src_0", "b"]);
    let b_stmt = find_var(&stmts, "b").unwrap();
    if let Statement::VarDeclaration {
        init: Some(Expression::Member { property: PropertyKey::String(k), .. }),
        ..
    } = b_stmt
    {
        assert_eq!(k, "1", "hole at index 0 means b is at index 1");
    }
}

#[test]
fn lower_array_destructure_rest() {
    let stmts = parse_statements("let [a, ...rest] = arr;");
    assert!(var_names(&stmts).contains(&"a".to_string()));
    assert!(var_names(&stmts).contains(&"rest".to_string()));
    assert!(var_names(&stmts).contains(&"__arr_src_0".to_string()));
}

#[test]
fn lower_array_destructure_nested() {
    let stmts = parse_statements("let [[a]] = nested;");
    let names = var_names(&stmts);
    assert!(names.contains(&"__arr_src_0".to_string()), "need src temp");
    assert!(names.contains(&"a".to_string()), "need 'a' declaration");
}

#[test]
fn lower_array_destructure_default() {
    let stmts = parse_statements("let [a = 1] = maybe;");
    let a_stmt = find_var(&stmts, "a").unwrap();
    if let Statement::VarDeclaration { init: Some(expr), .. } = a_stmt {
        assert!(matches!(expr, Expression::Binary { op: crate::ast::BinaryOp::NullishCoalescing, .. }),
            "expected NullishCoalescing for default, got {:?}", expr);
    } else {
        panic!("expected VarDeclaration for 'a'");
    }
}

#[test]
fn lower_array_destructure_empty() {
    let stmts = parse_statements("let [] = arr;");
    assert_eq!(var_names(&stmts), &["__arr_src_0"]);
}

// ─── Object Destructuring ────────────────────────────────────────────────

#[test]
fn lower_object_destructure_shorthand() {
    let _raw = parse_script("let {x, y} = obj;").unwrap();
    let stmts = parse_statements("let {x, y} = obj;");
    let names = var_names(&stmts);
    assert!(names.contains(&"x".to_string()), "expected 'x' in {:?}", names);
    assert!(names.contains(&"y".to_string()), "expected 'y' in {:?}", names);
}

#[test]
fn lower_object_destructure_renamed() {
    let stmts = parse_statements("let {x: y} = obj;");
    assert_eq!(var_names(&stmts), &["__obj_src_0", "y"]);
    assert!(!var_names(&stmts).contains(&"x".to_string()));
}

#[test]
fn lower_object_destructure_default() {
    let stmts = parse_statements("let {x = 5} = obj;");
    let x_stmt = find_var(&stmts, "x").unwrap();
    if let Statement::VarDeclaration { init: Some(expr), .. } = x_stmt {
        assert!(matches!(expr, Expression::Binary { op: crate::ast::BinaryOp::NullishCoalescing, .. }),
            "expected NullishCoalescing for default, got {:?}", expr);
    }
}

#[test]
fn lower_object_destructure_rest() {
    let stmts = parse_statements("let {a, ...rest} = obj;");
    let names = var_names(&stmts);
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"rest".to_string()));
}

#[test]
fn lower_object_destructure_nested_object() {
    let stmts = parse_statements("let {x: {y}} = obj;");
    let names = var_names(&stmts);
    assert!(names.contains(&"y".to_string()));
}

#[test]
fn lower_object_destructure_nested_array() {
    let stmts = parse_statements("let {x: [a, b]} = obj;");
    let names = var_names(&stmts);
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));
}

#[test]
fn lower_object_destructure_numeric_key() {
    let stmts = parse_statements("let {0: a} = arr;");
    let a_stmt = find_var(&stmts, "a").unwrap();
    if let Statement::VarDeclaration {
        init: Some(Expression::Member { property: PropertyKey::String(k), .. }),
        ..
    } = a_stmt
    {
        assert_eq!(k, "0");
    }
}

#[test]
fn lower_object_destructure_empty() {
    let stmts = parse_statements("let {} = obj;");
    assert_eq!(var_names(&stmts), &["__obj_src_0"]);
}

// ─── const / var destructuring ───────────────────────────────────────────

#[test]
fn lower_const_array_destructure() {
    let stmts = parse_statements("const [a] = arr;");
    assert_eq!(var_names(&stmts), &["__arr_src_0", "a"]);
    let a_stmt = find_var(&stmts, "a").unwrap();
    assert!(matches!(a_stmt, Statement::VarDeclaration { kind: VarKind::Const, .. }));
}

#[test]
fn lower_var_array_destructure() {
    let stmts = parse_statements("var [a] = arr;");
    assert_eq!(var_names(&stmts), &["__arr_src_0", "a"]);
    let a_stmt = find_var(&stmts, "a").unwrap();
    assert!(matches!(a_stmt, Statement::VarDeclaration { kind: VarKind::Var, .. }));
}

// ─── Function parameters (destructuring) ────────────────────────────────

#[test]
fn lower_param_array_destructure() {
    let stmts = parse_statements("function f([a, b]) {}");
    assert_eq!(stmts.len(), 1);
    if let Statement::FunctionDeclaration { params, .. } = &stmts[0] {
        let has_pattern = params.iter().any(|p| p.pattern.is_some());
        assert!(has_pattern, "destructuring param should have pattern set");
    }
}

#[test]
fn lower_param_object_destructure() {
    let stmts = parse_statements("function f({x}) {}");
    assert_eq!(stmts.len(), 1);
    if let Statement::FunctionDeclaration { params, .. } = &stmts[0] {
        let has_pattern = params.iter().any(|p| p.pattern.is_some());
        assert!(has_pattern, "object destructuring param should have pattern set");
    }
}

#[test]
fn lower_param_rest_array() {
    let stmts = parse_statements("function f(...args) {}");
    assert_eq!(stmts.len(), 1);
    if let Statement::FunctionDeclaration { params, .. } = &stmts[0] {
        let rest_param = params.iter().find(|p| p.rest);
        assert!(rest_param.is_some(), "rest parameter should be marked rest=true");
    }
}

#[test]
fn lower_param_rest_destructuring() {
    let stmts = parse_statements("function f(...[a, b]) {}");
    assert_eq!(stmts.len(), 1);
    if let Statement::FunctionDeclaration { params, .. } = &stmts[0] {
        let rest_param = params.iter().find(|p| p.rest);
        assert!(rest_param.is_some(), "rest param should exist");
        let rest = rest_param.unwrap();
        assert!(rest.pattern.is_some(), "rest with array pattern should have pattern");
    }
}

// ─── binding_to_expr (via for-in/for-of with destructuring) ─────────────

#[test]
fn lower_for_in_array_destructure() {
    let stmts = parse_statements("for ([a, b] in obj) {}");
    let for_in = match &stmts[0] {
        Statement::Expression(e) => match e.as_ref() {
            Expression::ForIn { variable, .. } => variable.as_ref(),
            _ => panic!("expected ForIn, got {:?}", e),
        },
        _ => panic!("expected Expression(ForIn), got {:?}", stmts[0]),
    };
    match for_in {
        Expression::ArrayPattern(elems) => {
            assert_eq!(elems.len(), 2);
            assert!(matches!(&elems[0], BindingElement::Identifier(id) if id == "a"));
            assert!(matches!(&elems[1], BindingElement::Identifier(id) if id == "b"));
        }
        _ => panic!("expected ArrayPattern, got {:?}", for_in),
    }
}

#[test]
fn lower_for_in_object_destructure() {
    let stmts = parse_statements("for ({x} in obj) {}");
    let for_in = match &stmts[0] {
        Statement::Expression(e) => match e.as_ref() {
            Expression::ForIn { variable, .. } => variable.as_ref(),
            _ => panic!("expected ForIn, got {:?}", e),
        },
        _ => panic!("expected Expression(ForIn), got {:?}", stmts[0]),
    };
    match for_in {
        Expression::ObjectPattern(props) => { assert_eq!(props.len(), 1); }
        _ => panic!("expected ObjectPattern, got {:?}", for_in),
    }
}

#[test]
fn lower_for_of_array_destructure() {
    let stmts = parse_statements("for ([a] of iter) {}");
    let for_of = match &stmts[0] {
        Statement::Expression(e) => match e.as_ref() {
            Expression::ForOf { variable, .. } => variable.as_ref(),
            _ => panic!("expected ForOf, got {:?}", e),
        },
        _ => panic!("expected Expression(ForOf), got {:?}", stmts[0]),
    };
    match for_of {
        Expression::ArrayPattern(elems) => {
            assert_eq!(elems.len(), 1);
            assert!(matches!(&elems[0], BindingElement::Identifier(id) if id == "a"));
        }
        _ => panic!("expected ArrayPattern, got {:?}", for_of),
    }
}

// ─── lower_prop_name_key variants ───────────────────────────────────────

#[test]
fn lower_object_destructure_string_key() {
    let stmts = parse_statements(r#"let {"x": a} = obj;"#);
    let a_stmt = find_var(&stmts, "a").unwrap();
    if let Statement::VarDeclaration { init: Some(expr), .. } = a_stmt {
        if let Expression::Member { property: PropertyKey::String(k), .. } = expr {
            assert_eq!(k, "x");
        } else {
            panic!("expected Member with string key 'x', got {:?}", expr);
        }
    }
}

#[test]
fn lower_object_destructure_boolean_key() {
    let stmts = parse_statements("let {true: a} = obj;");
    let a_stmt = find_var(&stmts, "a").unwrap();
    if let Statement::VarDeclaration {
        init: Some(Expression::Member { property: PropertyKey::String(k), .. }),
        ..
    } = a_stmt
    {
        assert_eq!(k, "true");
    }
}

// ─── Runtime correctness ─────────────────────────────────────────────────

#[test]
fn runtime_array_destructure_basic() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let [a, b] = [1, 2]; a + b").unwrap();
    assert_eq!(r, crate::value::Value::Number(3.0));
}

#[test]
fn runtime_array_destructure_rest() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let [first, ...rest] = [10, 20, 30]; first + rest.length").unwrap();
    assert_eq!(r, crate::value::Value::Number(12.0));
}

#[test]
fn runtime_array_destructure_default() {
    let ctx2 = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx2);
    let r = ctx2.eval("let [a = 99] = []; a").unwrap();
    assert_eq!(r, crate::value::Value::Number(99.0));

    let ctx3 = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx3);
    let r2 = ctx3.eval("let [a = 99] = [5]; a").unwrap();
    assert_eq!(r2, crate::value::Value::Number(5.0));
}

#[test]
fn runtime_array_destructure_hole() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let [, b] = [1, 2]; b").unwrap();
    assert_eq!(r, crate::value::Value::Number(2.0));
}

#[test]
fn runtime_object_destructure_basic() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let {x, y} = {x: 10, y: 20}; x + y").unwrap();
    assert_eq!(r, crate::value::Value::Number(30.0));
}

#[test]
fn runtime_object_destructure_renamed() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let {x: alias} = {x: 42}; alias").unwrap();
    assert_eq!(r, crate::value::Value::Number(42.0));
}

#[test]
fn runtime_object_destructure_default() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let {a = 7} = {}; a").unwrap();
    assert_eq!(r, crate::value::Value::Number(7.0));
}

#[test]
fn runtime_object_destructure_rest() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let {x, ...rest} = {x: 1, y: 2, z: 3}; rest.y + rest.z").unwrap();
    assert_eq!(r, crate::value::Value::Number(5.0));
}

#[test]
fn runtime_nested_destructure() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("let {x: [a, b]} = {x: [1, 2]}; a + b").unwrap();
    assert_eq!(r, crate::value::Value::Number(3.0));
}

#[test]
fn runtime_destructure_in_for_of() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval(
        r#"
        let sum = 0;
        for (let [x, y] of [[1,2],[3,4],[5,6]]) { sum += x + y; }
        sum
        "#,
    ).unwrap();
    assert_eq!(r, crate::value::Value::Number(21.0));
}

#[test]
fn runtime_destructure_param() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("function f([a, b]) { return a + b; } f([10, 20])").unwrap();
    assert_eq!(r, crate::value::Value::Number(30.0));
}

#[test]
fn runtime_destructure_param_object() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("function f({x, y}) { return x * y; } f({x: 6, y: 7})").unwrap();
    assert_eq!(r, crate::value::Value::Number(42.0));
}

#[test]
fn runtime_destructure_param_rest() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx.eval("function f(a, ...rest) { return rest.length; } f(1, 2, 3, 4)").unwrap();
    assert_eq!(r, crate::value::Value::Number(3.0));
}
