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

fn find_pattern_decl(stmts: &[Statement]) -> Option<&Statement> {
    stmts
        .iter()
        .find(|s| matches!(s, Statement::PatternDeclaration { .. }))
}

// ─── Array Destructuring ─────────────────────────────────────────────────

#[test]
fn lower_array_destructure_basic() {
    let stmts = parse_statements("let [a, b] = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration {
        kind: VarKind::Let,
        init: Some(expr),
        pattern,
    } = decl
    {
        assert!(matches!(expr, Expression::Identifier(i) if i == "arr"));
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert_eq!(elems.len(), 2);
            assert!(matches!(&elems[0], BindingElement::Identifier(n) if n == "a"));
            assert!(matches!(&elems[1], BindingElement::Identifier(n) if n == "b"));
        } else {
            panic!("expected ArrayPattern");
        }
    } else {
        panic!("expected PatternDeclaration(let)");
    }
}

#[test]
fn lower_array_destructure_with_hole() {
    let stmts = parse_statements("let [, b] = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert!(matches!(&elems[0], BindingElement::Identifier(n) if n == "__hole"));
            assert!(matches!(&elems[1], BindingElement::Identifier(n) if n == "b"));
        } else {
            panic!("expected ArrayPattern");
        }
    }
}

#[test]
fn lower_array_destructure_rest() {
    let stmts = parse_statements("let [a, ...rest] = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert!(matches!(&elems[0], BindingElement::Identifier(n) if n == "a"));
            assert!(matches!(&elems[1], BindingElement::Rest(_)));
        } else {
            panic!("expected ArrayPattern");
        }
    }
}

#[test]
fn lower_array_destructure_nested() {
    let stmts = parse_statements("let [[a]] = nested;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration {
        init: Some(expr),
        pattern,
        ..
    } = decl
    {
        assert!(matches!(expr, Expression::Identifier(i) if i == "nested"));
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert!(matches!(&elems[0], BindingElement::ArrayPattern(_)));
        } else {
            panic!("expected outer ArrayPattern");
        }
    }
}

#[test]
fn lower_array_destructure_default() {
    let stmts = parse_statements("let [a = 1] = maybe;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        assert!(matches!(pattern, BindingElement::ArrayPattern(_)));
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert!(matches!(&elems[0], BindingElement::Default(_, _)));
        }
    } else {
        panic!("expected PatternDeclaration");
    }
}

#[test]
fn lower_array_destructure_empty() {
    let stmts = parse_statements("let [] = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert!(elems.is_empty());
        } else {
            panic!("expected empty ArrayPattern");
        }
    }
}

// ─── Object Destructuring ────────────────────────────────────────────────

#[test]
fn lower_object_destructure_shorthand() {
    let stmts = parse_statements("let {x, y} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert_eq!(props.len(), 2);
            assert!(matches!(&props[0].1, BindingElement::Identifier(n) if n == "x"));
            assert!(matches!(&props[1].1, BindingElement::Identifier(n) if n == "y"));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_renamed() {
    let stmts = parse_statements("let {x: y} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert_eq!(props.len(), 1);
            assert!(matches!(&props[0].0, PropertyKey::Ident(k) if k == "x"));
            assert!(matches!(&props[0].1, BindingElement::Identifier(n) if n == "y"));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_default() {
    let stmts = parse_statements("let {x = 5} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert!(matches!(
                &props[0].1,
                BindingElement::Default(_, init) if matches!(init.as_ref(), Expression::Number(n) if *n == 5.0)
            ));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_rest() {
    let stmts = parse_statements("let {a, ...rest} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert_eq!(props.len(), 2);
            assert!(matches!(&props[0].1, BindingElement::Identifier(n) if n == "a"));
            assert!(matches!(&props[1].0, PropertyKey::Ident(k) if k == "..."));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_nested_object() {
    let stmts = parse_statements("let {x: {y}} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert!(matches!(
                &props[0].1,
                BindingElement::ObjectPattern(nested) if nested.len() == 1
                    && matches!(&nested[0].1, BindingElement::Identifier(n) if n == "y")
            ));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_nested_array() {
    let stmts = parse_statements("let {x: [a, b]} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert!(matches!(
                &props[0].1,
                BindingElement::ArrayPattern(elems) if elems.len() == 2
                    && matches!(&elems[0], BindingElement::Identifier(n) if n == "a")
                    && matches!(&elems[1], BindingElement::Identifier(n) if n == "b")
            ));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_numeric_key() {
    let stmts = parse_statements("let {0: a} = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert!(matches!(&props[0].0, PropertyKey::Number(n) if *n == 0.0));
            assert!(matches!(&props[0].1, BindingElement::Identifier(n) if n == "a"));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_empty() {
    let stmts = parse_statements("let {} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        assert!(matches!(pattern, BindingElement::ObjectPattern(props) if props.is_empty()));
    }
}

// ─── const / var destructuring ───────────────────────────────────────────

#[test]
fn lower_const_array_destructure() {
    let stmts = parse_statements("const [a] = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    assert!(matches!(
        decl,
        Statement::PatternDeclaration {
            kind: VarKind::Const,
            ..
        }
    ));
}

#[test]
fn lower_const_object_destructure() {
    let stmts = parse_statements("const {a} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    assert!(matches!(
        decl,
        Statement::PatternDeclaration {
            kind: VarKind::Const,
            ..
        }
    ));
}

#[test]
fn lower_var_array_destructure() {
    let stmts = parse_statements("var [a] = arr;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    assert!(matches!(
        decl,
        Statement::PatternDeclaration {
            kind: VarKind::Var,
            ..
        }
    ));
}

// ─── Function parameters (destructuring) ────────────────────────────────

#[test]
fn lower_class_method_array_elision() {
    let stmts = parse_statements("class C { method([,]) {} }");
    if let Statement::ClassDeclaration { class, .. } = &stmts[0] {
        let method = class
            .body
            .iter()
            .find_map(|m| {
                if let crate::ast::ClassMember::Method { params, .. } = m {
                    Some(params)
                } else {
                    None
                }
            })
            .expect("method");
        let pattern = method[0].pattern.as_ref().expect("pattern");
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert_eq!(elems.len(), 1);
            assert!(matches!(&elems[0], BindingElement::Identifier(n) if n == "__hole"));
        } else {
            panic!("expected array pattern");
        }
    }
}

#[test]
fn lower_param_array_elision() {
    let stmts = parse_statements("function f([,]) {}");
    if let Statement::FunctionDeclaration { params, .. } = &stmts[0] {
        let pattern = params[0].pattern.as_ref().expect("pattern");
        if let BindingElement::ArrayPattern(elems) = pattern {
            assert_eq!(elems.len(), 1);
            assert!(matches!(&elems[0], BindingElement::Identifier(n) if n == "__hole"));
        } else {
            panic!("expected array pattern, got {:?}", pattern);
        }
    }
}

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
        assert!(
            has_pattern,
            "object destructuring param should have pattern set"
        );
    }
}

#[test]
fn lower_param_object_destructure_computed_key() {
    let stmts = parse_statements("function f({ [k()]: x }) {}");
    assert_eq!(stmts.len(), 1);
    let Statement::FunctionDeclaration { params, .. } = &stmts[0] else {
        panic!("expected FunctionDeclaration");
    };
    let pattern = params[0].pattern.as_ref().expect("expected pattern");
    let BindingElement::ObjectPattern(props) = pattern else {
        panic!("expected ObjectPattern, got {:?}", pattern);
    };
    assert_eq!(props.len(), 1);
    assert!(
        matches!(&props[0].0, PropertyKey::Computed(_)),
        "computed key should be preserved, got {:?}",
        props[0].0
    );
}

#[test]
fn lower_param_rest_array() {
    let stmts = parse_statements("function f(...args) {}");
    assert_eq!(stmts.len(), 1);
    if let Statement::FunctionDeclaration { params, .. } = &stmts[0] {
        let rest_param = params.iter().find(|p| p.rest);
        assert!(
            rest_param.is_some(),
            "rest parameter should be marked rest=true"
        );
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
        assert!(
            rest.pattern.is_some(),
            "rest with array pattern should have pattern"
        );
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
        Expression::ObjectPattern(props) => {
            assert_eq!(props.len(), 1);
        }
        _ => panic!("expected ObjectPattern, got {:?}", for_in),
    }
}

#[test]
fn lower_for_of_object_id_init_default() {
    let stmts = parse_statements("for ({ a = 1 } of iter) {}");
    let for_of = match &stmts[0] {
        Statement::Expression(e) => match e.as_ref() {
            Expression::ForOf { variable, .. } => variable.as_ref(),
            _ => panic!("expected ForOf, got {:?}", e),
        },
        _ => panic!("expected Expression(ForOf), got {:?}", stmts[0]),
    };
    match for_of {
        Expression::ObjectPattern(props) => {
            assert!(matches!(
                &props[0].1,
                BindingElement::Default(_, init) if matches!(init.as_ref(), Expression::Number(n) if *n == 1.0)
            ));
        }
        _ => panic!("expected ObjectPattern, got {:?}", for_of),
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
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert!(matches!(&props[0].0, PropertyKey::String(k) if k == "x"));
            assert!(matches!(&props[0].1, BindingElement::Identifier(n) if n == "a"));
        } else {
            panic!("expected ObjectPattern");
        }
    }
}

#[test]
fn lower_object_destructure_boolean_key() {
    let stmts = parse_statements("let {true: a} = obj;");
    let decl = find_pattern_decl(&stmts).expect("expected PatternDeclaration");
    if let Statement::PatternDeclaration { pattern, .. } = decl {
        if let BindingElement::ObjectPattern(props) = pattern {
            assert!(matches!(&props[0].0, PropertyKey::String(k) if k == "true"));
            assert!(matches!(&props[0].1, BindingElement::Identifier(n) if n == "a"));
        } else {
            panic!("expected ObjectPattern");
        }
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
    let r = ctx
        .eval("let [first, ...rest] = [10, 20, 30]; first + rest.length")
        .unwrap();
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
    let r = ctx
        .eval("let {x, ...rest} = {x: 1, y: 2, z: 3}; rest.y + rest.z")
        .unwrap();
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
    let r = ctx
        .eval(
            r#"
        let sum = 0;
        for (let [x, y] of [[1,2],[3,4],[5,6]]) { sum += x + y; }
        sum
        "#,
        )
        .unwrap();
    assert_eq!(r, crate::value::Value::Number(21.0));
}

#[test]
fn lower_for_of_rest_nested_array_pattern() {
    let stmts = parse_statements("for (let [...[x, y, z]] of iter) {}");
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
            match &elems[0] {
                BindingElement::Rest(inner) => match inner.as_ref() {
                    BindingElement::ArrayPattern(nested) => {
                        assert_eq!(nested.len(), 3);
                    }
                    other => panic!("expected nested ArrayPattern, got {:?}", other),
                },
                other => panic!("expected Rest, got {:?}", other),
            }
        }
        _ => panic!("expected ArrayPattern, got {:?}", for_of),
    }
}

#[test]
fn runtime_for_of_rest_nested_array_pattern() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx
        .eval(
            "var iterCount = 0; \
             for (let [...[x, y, z]] of [[3, 4, 5]]) { \
               iterCount += (x === 3 && y === 4 && z === 5) ? 1 : 0; \
             } iterCount",
        )
        .unwrap();
    assert_eq!(r, crate::value::Value::Number(1.0));
}

#[test]
fn runtime_destructure_param() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx
        .eval("function f([a, b]) { return a + b; } f([10, 20])")
        .unwrap();
    assert_eq!(r, crate::value::Value::Number(30.0));
}

#[test]
fn runtime_destructure_param_object() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx
        .eval("function f({x, y}) { return x * y; } f({x: 6, y: 7})")
        .unwrap();
    assert_eq!(r, crate::value::Value::Number(42.0));
}

#[test]
fn runtime_destructure_param_rest() {
    let ctx = &mut crate::Context::new().unwrap();
    crate::builtins::register_builtins(ctx);
    let r = ctx
        .eval("function f(a, ...rest) { return rest.length; } f(1, 2, 3, 4)")
        .unwrap();
    assert_eq!(r, crate::value::Value::Number(3.0));
}
