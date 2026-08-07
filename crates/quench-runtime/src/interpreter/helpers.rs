//! Private helper functions for the interpreter.
//! All functions here are internal helpers; public API lives in the parent `interpreter.rs`.

use crate::ast::*;
use crate::env::{Environment, Scope};
use crate::value::{Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

/// Check if the first statement is "use strict"; directive
pub fn check_use_strict_directive(statements: &[crate::ast::Statement]) -> bool {
    if let Some(crate::ast::Statement::Expression(expr)) = statements.first() {
        if let crate::ast::Expression::String(s) = expr.as_ref() {
            return s.trim() == "use strict";
        }
    }
    false
}

/// Set the `this` binding in the environment
pub fn set_this_binding(env: &Rc<RefCell<Environment>>, this_value: Value) {
    env.borrow_mut()
        .current_scope()
        .borrow_mut()
        .set_this_value(this_value);
}

/// Get the `this` binding from the environment chain
pub fn get_this_binding(env: &Rc<RefCell<Environment>>) -> Value {
    let mut current: Option<Rc<RefCell<Environment>>> = Some(Rc::clone(env));
    while let Some(e) = current {
        for scope_rc in e.borrow().scopes.iter().rev() {
            let scope = scope_rc.borrow();
            if let Some(this_val) = scope.get_this() {
                if !crate::interpreter::is_strict_mode()
                    && (this_val == Value::Undefined || this_val == Value::Null)
                {
                    let global_this = e.borrow().get("globalThis").unwrap_or(Value::Undefined);
                    return global_this;
                }
                return this_val;
            }
        }
        current = e.borrow().get_parent();
    }
    if !crate::interpreter::is_strict_mode() {
        let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
        return global_this;
    }
    Value::Undefined
}

/// Scope holding the lexical `this` binding (not necessarily `current_scope()`).
pub fn find_this_scope(env: &Rc<RefCell<Environment>>) -> Option<Rc<RefCell<Scope>>> {
    let mut current: Option<Rc<RefCell<Environment>>> = Some(Rc::clone(env));
    while let Some(e) = current {
        for scope_rc in e.borrow().scopes.iter().rev() {
            if scope_rc.borrow().get_this().is_some() {
                return Some(Rc::clone(scope_rc));
            }
        }
        current = e.borrow().get_parent();
    }
    None
}

pub fn is_this_binding_initialized(env: &Rc<RefCell<Environment>>) -> bool {
    find_this_scope(env).is_some_and(|s| s.borrow().is_this_initialized())
}

pub fn mark_this_binding_initialized(env: &Rc<RefCell<Environment>>) {
    if let Some(scope) = find_this_scope(env) {
        scope.borrow_mut().mark_this_initialized();
    }
}

pub fn set_this_binding_value(env: &Rc<RefCell<Environment>>, value: Value) {
    if let Some(scope) = find_this_scope(env) {
        scope.borrow_mut().set_this(value);
    }
}

/// Hoist function declarations to the top of the script/function scope
pub fn hoist_functions(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::FunctionDeclaration {
                name,
                params,
                body,
                is_async,
                is_generator,
            } => {
                let mut func = ValueFunction::new(
                    Some(name.clone()),
                    params.clone(),
                    body.clone(),
                    Rc::clone(env),
                    *is_async,
                    *is_generator,
                );
                func.strict = crate::interpreter::is_strict_mode();
                env.borrow_mut().define(name.clone(), Value::Function(func));
            }
            Statement::Block(stmts) => hoist_functions(stmts, env),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                hoist_functions(std::slice::from_ref(consequent.as_ref()), env);
                if let Some(alt) = alternate {
                    hoist_functions(std::slice::from_ref(alt.as_ref()), env);
                }
            }
            Statement::While { body, .. } => {
                hoist_functions(std::slice::from_ref(body.as_ref()), env)
            }
            Statement::For { body, .. } => {
                hoist_functions(std::slice::from_ref(body.as_ref()), env)
            }
            _ => {}
        }
    }
}

/// Hoist class declarations (block-scoped via `let`)
pub fn hoist_classes(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::ClassDeclaration { name, .. } => {
                env.borrow_mut().declare_var(name.clone(), VarKind::Let);
            }
            Statement::Block(stmts) => hoist_classes(stmts, env),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                hoist_classes(std::slice::from_ref(consequent.as_ref()), env);
                if let Some(alt) = alternate {
                    hoist_classes(std::slice::from_ref(alt.as_ref()), env);
                }
            }
            Statement::While { body, .. } => {
                hoist_classes(std::slice::from_ref(body.as_ref()), env)
            }
            Statement::For { body, .. } => hoist_classes(std::slice::from_ref(body.as_ref()), env),
            _ => {}
        }
    }
}

/// Collect all `var` names from statements (for hoisting)
pub fn collect_var_names(stmts: &[Statement]) -> Vec<String> {
    let mut names = Vec::new();
    collect_var_names_recursive(stmts, &mut names);
    names.sort();
    names.dedup();
    names
}

#[allow(clippy::complexity)]
pub fn collect_var_names_recursive(stmts: &[Statement], names: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            Statement::VarDeclaration {
                kind: VarKind::Var,
                name,
                ..
            } => {
                names.push(name.clone());
            }
            Statement::PatternDeclaration {
                kind: VarKind::Var,
                pattern,
                ..
            } => {
                names.extend(crate::lower::pattern::collect_pattern_identifiers(pattern));
            }
            Statement::Block(inner_stmts) => collect_var_names_recursive(inner_stmts, names),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                collect_var_names_recursive(std::slice::from_ref(consequent.as_ref()), names);
                if let Some(alt) = alternate {
                    collect_var_names_recursive(std::slice::from_ref(alt.as_ref()), names);
                }
            }
            Statement::While { body, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names)
            }
            Statement::For { body, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names)
            }
            Statement::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
                if let Some(h) = handler {
                    collect_var_names_recursive(std::slice::from_ref(h.as_ref()), names);
                }
                if let Some(f) = finalizer {
                    collect_var_names_recursive(std::slice::from_ref(f.as_ref()), names);
                }
            }
            Statement::SequenceDecls(inner) => collect_var_names_recursive(inner, names),
            Statement::ForIn { variable, body, .. } => {
                if let Expression::Identifier(name) = variable.as_ref() {
                    names.push(name.clone());
                }
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
            }
            Statement::Expression(expr) => {
                collect_var_names_from_expr(expr, names);
            }
            _ => {}
        }
    }
}

fn collect_var_names_from_expr(expr: &Expression, names: &mut Vec<String>) {
    match expr {
        Expression::ForIn { variable, body, .. } => {
            if let Expression::Identifier(name) = variable.as_ref() {
                names.push(name.clone());
            }
            collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
        }
        Expression::ForOf { variable, body, .. } => {
            if let Expression::Identifier(name) = variable.as_ref() {
                names.push(name.clone());
            }
            collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
        }
        _ => {}
    }
}

/// Collect all `let`/`const` declarations from statements
pub fn collect_let_const_declarations(stmts: &[Statement]) -> Vec<(String, VarKind)> {
    let mut decls = Vec::new();
    collect_let_const_recursive(stmts, &mut decls);
    decls.sort_by(|a, b| a.0.cmp(&b.0));
    decls.dedup_by(|a, b| a.0 == b.0);
    decls
}

pub fn collect_let_const_recursive(stmts: &[Statement], decls: &mut Vec<(String, VarKind)>) {
    for stmt in stmts {
        match stmt {
            Statement::VarDeclaration {
                kind: VarKind::Let,
                name,
                ..
            } => {
                decls.push((name.clone(), VarKind::Let));
            }
            Statement::VarDeclaration {
                kind: VarKind::Const,
                name,
                ..
            } => {
                decls.push((name.clone(), VarKind::Const));
            }
            Statement::PatternDeclaration {
                kind: VarKind::Let,
                pattern,
                ..
            } => {
                for name in crate::lower::pattern::collect_pattern_identifiers(pattern) {
                    decls.push((name, VarKind::Let));
                }
            }
            Statement::PatternDeclaration {
                kind: VarKind::Const,
                pattern,
                ..
            } => {
                for name in crate::lower::pattern::collect_pattern_identifiers(pattern) {
                    decls.push((name, VarKind::Const));
                }
            }
            Statement::SequenceDecls(inner) => collect_let_const_recursive(inner, decls),
            _ => {}
        }
    }
}

/// Predeclare `var` bindings in the environment
pub fn predeclare_var(stmts: &[Statement], env: &mut Environment) {
    let names = collect_var_names(stmts);
    for name in names {
        env.declare_var(name, VarKind::Var);
    }
}

/// Predeclare `let`/`const` bindings in the environment
pub fn predeclare_let_const(stmts: &[Statement], env: &mut Environment) {
    let decls = collect_let_const_declarations(stmts);
    for (name, kind) in decls {
        if !env.current_scope().borrow().has(&name) {
            env.declare_var(name, kind);
        }
    }
}

/// Returns true if `source` contains a legacy octal literal (e.g. `01`, `07`).
/// Skips strings, template literals, comments, and regex literals.
pub fn has_legacy_octal(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // Skip string literals
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] == b'u' {
                        i += 2;
                        for _ in 0..4 {
                            if i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                                i += 1;
                            } else {
                                break;
                            }
                        }
                    } else {
                        i += 2;
                    }
                    continue;
                }
                i += 1;
            }
            continue;
        }
        // Skip template literals
        if b == b'`' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'`' {
                    i += 1;
                    break;
                }
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] == b'u' {
                        i += 2;
                        for _ in 0..4 {
                            if i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                                i += 1;
                            } else {
                                break;
                            }
                        }
                    } else {
                        i += 2;
                    }
                    continue;
                }
                i += 1;
            }
            continue;
        }
        // Skip line comments
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip block comments
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] == b'u' {
                        i += 2;
                        if i < bytes.len() && bytes[i] == b'{' {
                            i += 1;
                            while i < bytes.len() && bytes[i] != b'}' {
                                i += 1;
                            }
                            if i < bytes.len() {
                                i += 1;
                            }
                        } else {
                            for _ in 0..4 {
                                if i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                                    i += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                        continue;
                    }
                    i += 2;
                    continue;
                }
                i += 1;
            }
            continue;
        }
        // Skip regex literals
        if b == b'/' {
            let at_regex_start = if i == 0 {
                true
            } else {
                let prev = bytes[i - 1];
                matches!(
                    prev,
                    b'(' | b'['
                        | b'{'
                        | b'='
                        | b','
                        | b';'
                        | b'+'
                        | b'-'
                        | b' '
                        | b'\t'
                        | b'\n'
                        | b'\r'
                ) || matches!(prev, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$' | b')' | b']')
            };
            if at_regex_start && i + 1 < bytes.len() && bytes[i + 1] != b'/' && bytes[i + 1] != b'*'
            {
                i += 1;
                while i < bytes.len() {
                    let rb = bytes[i];
                    if rb == b'/' {
                        i += 1;
                        break;
                    }
                    if rb == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if rb == b'[' {
                        i += 1;
                        while i < bytes.len() {
                            let cb = bytes[i];
                            if cb == b']' {
                                i += 1;
                                break;
                            }
                            if cb == b'\\' && i + 1 < bytes.len() {
                                i += 2;
                                continue;
                            }
                            if cb == b'\n' || cb == b'\r' {
                                break;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    if rb == b'\n' || rb == b'\r' {
                        break;
                    }
                    i += 1;
                }
                continue;
            }
        }
        // Check for legacy octal: `0` followed by `1-7`
        if b == b'0' {
            if i + 2 < bytes.len() && bytes[i + 1] == b'u' {
                if bytes[i + 2] == b'{' {
                    i += 3;
                    while i < bytes.len() && bytes[i] != b'}' {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                    continue;
                } else {
                    i += 2;
                    if i + 3 < bytes.len()
                        && bytes[i].is_ascii_hexdigit()
                        && bytes[i + 1].is_ascii_hexdigit()
                        && bytes[i + 2].is_ascii_hexdigit()
                        && bytes[i + 3].is_ascii_hexdigit()
                    {
                        i += 4;
                        continue;
                    }
                    i -= 2;
                }
            }
            if i + 2 < bytes.len() && bytes[i + 1] == b'x' {
                i += 2;
                if bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                continue;
            }
            let rest = &bytes[i + 1..];
            if let Some(&next) = rest.first() {
                if next == b'8' || next == b'9' {
                    i += 1;
                    continue;
                }
                if matches!(next, b'1'..=b'7') && rest.len() >= 2 {
                    let after = rest[1];
                    if after == b'8' || after == b'9' {
                        i += 3;
                        continue;
                    }
                    let mut j = 1;
                    while j < rest.len() && matches!(rest[j], b'0'..=b'7') {
                        j += 1;
                    }
                    if j < rest.len() && (rest[j] == b'8' || rest[j] == b'9') {
                        i += 1 + j;
                        continue;
                    }
                }
                let prev_is_non_octal = i > 0
                    && matches!(
                        bytes[i - 1],
                        b'0'..=b'9' | b'.' | b'\'' | b'"' | b'e' | b'E' | b'+' | b'-' | b'_'
                    );
                let prev_is_xbo =
                    i > 0 && matches!(bytes[i - 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O');
                let prev_is_utf8_cont = i > 0 && (bytes[i - 1] & 0x80) != 0;
                if !prev_is_non_octal
                    && !prev_is_xbo
                    && !prev_is_utf8_cont
                    && next != b'n'
                    && next != b'e'
                    && next != b'E'
                    && matches!(next, b'1'..=b'7')
                {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expression, Statement, VarKind};

    // ── check_use_strict_directive ───────────────────────────────────────────────

    #[test]
    fn test_check_use_strict_true() {
        let stmts = vec![Statement::Expression(Box::new(Expression::String(
            "use strict".to_string(),
        )))];
        assert!(crate::interpreter::helpers::check_use_strict_directive(
            &stmts
        ));
    }

    #[test]
    fn test_check_use_strict_with_whitespace() {
        let stmts = vec![Statement::Expression(Box::new(Expression::String(
            "  use strict  ".to_string(),
        )))];
        assert!(crate::interpreter::helpers::check_use_strict_directive(
            &stmts
        ));
    }

    #[test]
    fn test_check_use_strict_false_number() {
        let stmts = vec![Statement::Expression(Box::new(Expression::Number(1.0)))];
        assert!(!crate::interpreter::helpers::check_use_strict_directive(
            &stmts
        ));
    }

    #[test]
    fn test_check_use_strict_false_identifier() {
        let stmts = vec![Statement::Expression(Box::new(Expression::Identifier(
            "use strict".to_string(),
        )))];
        assert!(!crate::interpreter::helpers::check_use_strict_directive(
            &stmts
        ));
    }

    #[test]
    fn test_check_use_strict_empty() {
        let stmts: Vec<Statement> = vec![];
        assert!(!crate::interpreter::helpers::check_use_strict_directive(
            &stmts
        ));
    }

    // ── collect_var_names ────────────────────────────────────────────────────────

    #[test]
    fn test_collect_var_names_simple() {
        let stmts = vec![Statement::VarDeclaration {
            kind: VarKind::Var,
            name: "x".to_string(),
            init: None,
        }];
        let names = crate::interpreter::helpers::collect_var_names(&stmts);
        assert!(names.contains(&"x".to_string()));
    }

    #[test]
    fn test_collect_var_names_ignores_let() {
        let stmts = vec![Statement::VarDeclaration {
            kind: VarKind::Let,
            name: "y".to_string(),
            init: None,
        }];
        let names = crate::interpreter::helpers::collect_var_names(&stmts);
        assert!(!names.contains(&"y".to_string()));
    }

    #[test]
    fn test_collect_var_names_ignores_const() {
        let stmts = vec![Statement::VarDeclaration {
            kind: VarKind::Const,
            name: "z".to_string(),
            init: None,
        }];
        let names = crate::interpreter::helpers::collect_var_names(&stmts);
        assert!(!names.contains(&"z".to_string()));
    }

    #[test]
    fn test_collect_var_names_deduplicates() {
        let stmts = vec![
            Statement::VarDeclaration {
                kind: VarKind::Var,
                name: "x".to_string(),
                init: None,
            },
            Statement::VarDeclaration {
                kind: VarKind::Var,
                name: "x".to_string(),
                init: None,
            },
        ];
        let names = crate::interpreter::helpers::collect_var_names(&stmts);
        assert_eq!(names.iter().filter(|n| *n == "x").count(), 1);
    }

    #[test]
    fn test_collect_var_names_nested() {
        let stmts = vec![Statement::Block(vec![Statement::VarDeclaration {
            kind: VarKind::Var,
            name: "nested".to_string(),
            init: None,
        }])];
        let names = crate::interpreter::helpers::collect_var_names(&stmts);
        assert!(names.contains(&"nested".to_string()));
    }

    // ── collect_let_const_declarations ──────────────────────────────────────────

    #[test]
    fn test_collect_let_const_let() {
        let stmts = vec![Statement::VarDeclaration {
            kind: VarKind::Let,
            name: "a".to_string(),
            init: None,
        }];
        let decls = crate::interpreter::helpers::collect_let_const_declarations(&stmts);
        assert!(decls.contains(&("a".to_string(), VarKind::Let)));
    }

    #[test]
    fn test_collect_let_const_const() {
        let stmts = vec![Statement::VarDeclaration {
            kind: VarKind::Const,
            name: "b".to_string(),
            init: None,
        }];
        let decls = crate::interpreter::helpers::collect_let_const_declarations(&stmts);
        assert!(decls.contains(&("b".to_string(), VarKind::Const)));
    }

    #[test]
    fn test_collect_let_const_ignores_var() {
        let stmts = vec![Statement::VarDeclaration {
            kind: VarKind::Var,
            name: "c".to_string(),
            init: None,
        }];
        let decls = crate::interpreter::helpers::collect_let_const_declarations(&stmts);
        assert!(decls.is_empty());
    }

    #[test]
    fn test_collect_let_const_deduplicates() {
        let stmts = vec![
            Statement::VarDeclaration {
                kind: VarKind::Let,
                name: "dup".to_string(),
                init: None,
            },
            Statement::VarDeclaration {
                kind: VarKind::Let,
                name: "dup".to_string(),
                init: None,
            },
        ];
        let decls = crate::interpreter::helpers::collect_let_const_declarations(&stmts);
        assert_eq!(decls.len(), 1);
    }

    // ── has_legacy_octal ────────────────────────────────────────────────────────

    #[test]
    fn test_has_legacy_octal_basic() {
        assert!(crate::interpreter::helpers::has_legacy_octal("01"));
        assert!(crate::interpreter::helpers::has_legacy_octal("07"));
        assert!(crate::interpreter::helpers::has_legacy_octal("010"));
    }

    #[test]
    fn test_has_legacy_octal_zero_alone() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("0"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("0;"));
        assert!(!crate::interpreter::helpers::has_legacy_octal(""));
    }

    #[test]
    fn test_has_legacy_octal_hex_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("0xFF"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("0x1"));
    }

    #[test]
    fn test_has_legacy_octal_binary_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("0b01"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("0B1"));
    }

    #[test]
    fn test_has_legacy_octal_after_digit() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("10"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("20"));
    }

    #[test]
    fn test_has_legacy_octal_in_string_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("\"01\""));
        assert!(!crate::interpreter::helpers::has_legacy_octal("'07'"));
    }

    #[test]
    fn test_has_legacy_octal_in_template_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("`01`"));
    }

    #[test]
    fn test_has_legacy_octal_in_comment_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal(
            "// 01\nvar x;"
        ));
        assert!(!crate::interpreter::helpers::has_legacy_octal("/* 07 */"));
    }

    #[test]
    fn test_has_legacy_octal_regex_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("/01/"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("x = /01/"));
    }

    #[test]
    fn test_has_legacy_octal_8_9_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("08"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("09"));
    }

    #[test]
    fn test_has_legacy_octal_after_e_ignored() {
        assert!(!crate::interpreter::helpers::has_legacy_octal("1e01"));
        assert!(!crate::interpreter::helpers::has_legacy_octal("1E07"));
    }

    #[test]
    fn test_has_legacy_octal_n_suffix_ignored() {
        // 0n (BigInt zero) should not be flagged as octal
        assert!(!crate::interpreter::helpers::has_legacy_octal("0n"));
        // But 01n is still legacy octal (01 before the n)
        assert!(crate::interpreter::helpers::has_legacy_octal("01n"));
    }
}
