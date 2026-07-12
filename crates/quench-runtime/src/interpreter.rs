//! JavaScript interpreter - evaluates AST nodes
//!
//! This module provides the main interpreter entry points. The actual evaluation
//! logic is in the `eval` module.

use crate::ast::*;
use crate::env::Environment;
use crate::value::{JsError, Object, Value};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Control flow for break/continue/return statements
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ControlFlow {
    Break,
    Continue,
    Return(Value),
}

thread_local! {
    static CONTROL_FLOW: Cell<Option<ControlFlow>> = const { Cell::new(None) };
}

pub(crate) fn set_control_flow(cf: ControlFlow) {
    CONTROL_FLOW.with(|cell| cell.set(Some(cf)));
}

pub(crate) fn take_control_flow() -> Option<ControlFlow> {
    CONTROL_FLOW.with(|cell| cell.take())
}

#[allow(dead_code)]
pub(crate) fn is_control_flow_set() -> bool {
    CONTROL_FLOW.with(|cell| {
        let val = cell.take();
        let is_set = val.is_some();
        // Restore so eval_statements / loops can consume it
        cell.set(val);
        is_set
    })
}

const DEFAULT_MAX_RECURSION_DEPTH: usize = 10000;
static MAX_RECURSION_DEPTH_OVERRIDE: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_RECURSION_DEPTH);

fn get_max_depth() -> usize {
    MAX_RECURSION_DEPTH_OVERRIDE.load(Ordering::SeqCst)
}

#[allow(dead_code)]
pub fn set_max_call_depth(depth: usize) {
    MAX_RECURSION_DEPTH_OVERRIDE.store(depth, Ordering::SeqCst);
}

thread_local! {
    static CURRENT_THIS: Cell<Option<Value>> = const { Cell::new(None) };
}

thread_local! {
    static CALL_THIS: Cell<Option<Value>> = const { Cell::new(None) };
}

thread_local! {
    static CURRENT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
    static SUPER_CLASS: RefCell<Option<Value>> = const { RefCell::new(None) };
}

thread_local! {
    static STRICT_MODE: Cell<bool> = const { Cell::new(false) };
}

/// Check if we're currently in strict mode
pub(crate) fn is_strict_mode() -> bool {
    STRICT_MODE.with(|cell| cell.get())
}

/// Set strict mode (used when evaluating "use strict"; directives)
pub(crate) fn set_strict_mode(strict: bool) {
    STRICT_MODE.with(|cell| cell.set(strict));
}

/// Get the current superclass
pub(crate) fn get_super_class() -> Option<Value> {
    SUPER_CLASS.with(|cell| cell.borrow().clone())
}

/// Get the super prototype for the current class
pub fn get_super_prototype() -> Option<Rc<RefCell<Object>>> {
    get_super_class().and_then(|v| {
        if let Value::Function(ref f) = v {
            Some(f.get_prototype())
        } else if let Value::Object(ref o) = v {
            o.borrow().get("prototype").and_then(|p| {
                if let Value::Object(ref proto) = p {
                    Some(proto.clone())
                } else {
                    None
                }
            })
        } else {
            None
        }
    })
}

pub(crate) fn set_native_this(this_val: Value) {
    CURRENT_THIS.with(|cell| cell.set(Some(this_val)));
}

pub(crate) fn get_native_this() -> Option<Value> {
    CURRENT_THIS.with(|cell| {
        let val = cell.take();
        // Restore the value for subsequent calls
        cell.set(val.clone());
        val
    })
}

pub(crate) fn take_native_this() {
    CURRENT_THIS.with(|cell| {
        cell.take();
    });
}

pub(crate) fn set_this_value(this_val: Value) {
    CALL_THIS.with(|cell| cell.set(Some(this_val)));
}

pub(crate) fn get_this_value() -> Option<Value> {
    CALL_THIS.with(|cell| {
        let val = cell.take();
        cell.set(val.clone());
        val
    })
}

pub(crate) fn take_this_value() {
    CALL_THIS.with(|cell| {
        cell.take();
    });
}

pub(crate) fn check_depth() -> Result<(), JsError> {
    let depth = CURRENT_DEPTH.with(|cell| {
        let d = cell.get();
        cell.set(d + 1);
        d
    });
    if depth >= get_max_depth() {
        CURRENT_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
        Err(JsError("Maximum call stack size exceeded".to_string()))
    } else {
        Ok(())
    }
}

pub(crate) fn release_depth() {
    CURRENT_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
}

/// RAII guard that releases the recursion depth counter when dropped
pub(crate) struct DepthGuard;

/// Check the recursion depth, returning a guard that releases it on drop
pub(crate) fn check_depth_guard() -> Result<DepthGuard, JsError> {
    check_depth()?;
    Ok(DepthGuard)
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        release_depth();
    }
}

pub fn reset_depth() {
    CURRENT_DEPTH.with(|cell| cell.set(0));
}

/// Evaluate a complete program with hoisting
pub fn eval_program(
    program: &Program,
    env: &mut Rc<RefCell<Environment>>,
    source: Option<&str>,
) -> Result<Value, JsError> {
    match program {
        Program::Script(statements) => {
            // Check for "use strict"; directive at the beginning of the script
            let prev_strict = is_strict_mode();
            let script_is_strict = check_use_strict_directive(statements);
            // Strict mode is inherited from the calling context (eval inherits strict
            // mode from its enclosing code per ECMAScript spec), OR from the script itself.
            let eval_is_strict = script_is_strict || is_strict_mode();
            set_strict_mode(eval_is_strict);

            // Legacy octal check is handled in eval_impl before the eval string is
            // parsed, so we inherit the correct strict mode from the outer context.
            // (eval_program is only called via ctx.eval, never for top-level scripts.)

            hoist_functions(statements, env);
            hoist_classes(statements, env);
            predeclare_var(statements, &mut env.borrow_mut());
            predeclare_let_const(statements, &mut env.borrow_mut());
            let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
            set_this_binding(env, global_this);
            let mut last_value = Value::Undefined;
            for stmt in statements {
                last_value = crate::eval::eval_statement(stmt, env, false, false)?;
            }

            // Restore previous strict mode
            set_strict_mode(prev_strict);

            // A top-level `return` is illegal JS; discard any stale control
            // flow so it cannot leak into the next eval call.
            let _ = take_control_flow();

            Ok(last_value)
        }
    }
}

/// Returns true if `source` contains a legacy octal literal (e.g. `01`, `07`).
///
/// Uses a quote/comment-aware scanner: skips over string literals, template
/// literals, and comments before scanning. Then flags `0` followed by `1-7`
/// when NOT preceded by a digit (avoids `2015`), `.` (avoids `1.07`), or a
/// quote (avoids `"%01"` in strings).
pub(crate) fn has_legacy_octal(source: &str) -> bool {
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
                // Skip Unicode escape sequences in block comments
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] == b'u' {
                        i += 2;
                        if i < bytes.len() && bytes[i] == b'{' {
                            // \u{...} form
                            i += 1;
                            while i < bytes.len() && bytes[i] != b'}' {
                                i += 1;
                            }
                            if i < bytes.len() {
                                i += 1;
                            }
                        } else {
                            // \uXXXX form — skip 4 hex digits
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
        // Regex-literal skip: '/' at a position that can start a regex.
        // Unambiguous: after `=`, `(`, `[`, `{`, `+`, `-`, `,`, `;` or whitespace.
        // Ambiguous: after id/number chars (identifiers, ')', ']') — still treated
        // as regex because division here is extremely rare (would need `(x) / y`).
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
                ) || matches!(prev, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
                        | b'_' | b'$' | b')' | b']')
            };
            if at_regex_start && i + 1 < bytes.len() && bytes[i + 1] != b'/' && bytes[i + 1] != b'*'
            {
                // Skip to closing '/', handling \[...\] and \\...
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
        // Check for legacy octal: `0` followed by `1-7` (but NOT `8` or `9`)
        if b == b'0' {
            // Skip '0' if it's part of a Unicode or hex escape sequence
            if i + 2 < bytes.len() && bytes[i + 1] == b'u' {
                if bytes[i + 2] == b'{' {
                    // \u{...} form — skip to '}'
                    i += 3;
                    while i < bytes.len() && bytes[i] != b'}' {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                    continue;
                } else {
                    // \uXXXX form — only skip if followed by 4 hex digits
                    i += 2; // past '\' and 'u'
                    if i + 3 < bytes.len()
                        && bytes[i].is_ascii_hexdigit()
                        && bytes[i + 1].is_ascii_hexdigit()
                        && bytes[i + 2].is_ascii_hexdigit()
                        && bytes[i + 3].is_ascii_hexdigit()
                    {
                        // Valid \uXXXX — skip 4 hex digits; `continue` skips outer `i += 1`
                        i += 4;
                        continue;
                    }
                    // Not a valid \uXXXX. Back up so outer loop processes '0' normally.
                    i -= 2;
                }
            }
            if i + 2 < bytes.len() && bytes[i + 1] == b'x' {
                // \xNN form — skip 2 hex digits
                i += 2;
                if bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                continue;
            }
            let rest = &bytes[i + 1..];
            if let Some(&next) = rest.first() {
                // If next is 8 or 9, skip the '0' (non-octal decimal like 08, 09)
                if next == b'8' || next == b'9' {
                    i += 1;
                    continue;
                }
                // If next is 1-7 and the char after that is 8 or 9, this is a non-octal
                // decimal like 018 or 019. Skip all three to avoid false positive on "01".
                if matches!(next, b'1'..=b'7') && rest.len() >= 2 {
                    let after = rest[1];
                    if after == b'8' || after == b'9' {
                        i += 3; // skip '0', '1-7', and '8/9'
                        continue;
                    }
                    // Scan all remaining consecutive octal digits, then check for 8/9.
                    // This handles "0708" where "070" precedes "8": all three chars
                    // must be skipped so the whole thing is treated as NonOctalDecimalIntegerLiteral.
                    let mut j = 1;
                    while j < rest.len() && matches!(rest[j], b'0'..=b'7') {
                        j += 1;
                    }
                    if j < rest.len() && (rest[j] == b'8' || rest[j] == b'9') {
                        i += 1 + j; // skip '0' plus all octal digits
                        continue;
                    }
                }
                // Skip if preceded by digit, '.', quote, 'e'/'E' (exponent indicator),
                // '+'/'-' (exponent sign), or '_' (numeric separator)
                let prev_is_non_octal = i > 0
                    && matches!(
                        bytes[i - 1],
                        b'0'..=b'9' | b'.' | b'\'' | b'"' | b'e' | b'E' | b'+' | b'-' | b'_'
                    );
                let prev_is_xbo =
                    i > 0 && matches!(bytes[i - 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O');
                // Skip if this '0' is a UTF-8 continuation byte (high bit set means
                // it's part of a multi-byte sequence, not a standalone ASCII '0')
                let prev_is_utf8_cont = i > 0 && (bytes[i - 1] & 0x80) != 0;
                if !prev_is_non_octal
                    && !prev_is_xbo
                    && !prev_is_utf8_cont
                    && next != b'n'
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

/// Check if the first statement is "use strict"; directive
fn check_use_strict_directive(statements: &[crate::ast::Statement]) -> bool {
    if let Some(crate::ast::Statement::Expression(expr)) = statements.first() {
        if let crate::ast::Expression::String(s) = expr.as_ref() {
            return s.trim() == "use strict";
        }
    }
    false
}

pub(crate) fn set_this_binding(env: &Rc<RefCell<Environment>>, this_value: Value) {
    env.borrow_mut().current_scope_mut().set_this(this_value);
}

pub(crate) fn get_this_binding(env: &Rc<RefCell<Environment>>) -> Value {
    for scope in env.borrow().scopes.iter().rev() {
        if let Some(this_val) = scope.get_this() {
            // Sloppy mode: undefined/null this → globalThis (ESMA-262 12.2.1.1)
            if !is_strict_mode() && (this_val == Value::Undefined || this_val == Value::Null) {
                let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
                return global_this;
            }
            return this_val;
        }
    }
    if !is_strict_mode() {
        let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
        return global_this;
    }
    Value::Undefined
}

pub(crate) fn hoist_functions(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::FunctionDeclaration { name, params, body } => {
                let mut func = crate::value::ValueFunction::new(
                    Some(name.clone()),
                    params.clone(),
                    body.clone(),
                    Rc::clone(env),
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

pub(crate) fn hoist_classes(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::ClassDeclaration { name, class: _ } => {
                // Create class value placeholder for hoisting
                // The actual class is evaluated when the statement is executed
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

pub(crate) fn collect_var_names(stmts: &[Statement]) -> Vec<String> {
    let mut names = Vec::new();
    collect_var_names_recursive(stmts, &mut names);
    names.sort();
    names.dedup();
    names
}

#[allow(clippy::complexity)]
pub(crate) fn collect_var_names_recursive(stmts: &[Statement], names: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            Statement::VarDeclaration {
                kind: VarKind::Var,
                name,
                ..
            } => {
                names.push(name.clone());
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
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
            }
            Statement::For { body, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
            }
            Statement::TryCatch { body, handler, .. } => {
                collect_var_names_recursive(std::slice::from_ref(body.as_ref()), names);
                collect_var_names_recursive(std::slice::from_ref(handler.as_ref()), names);
            }
            Statement::SequenceDecls(inner) => {
                collect_var_names_recursive(inner, names);
            }
            _ => {}
        }
    }
}

pub(crate) fn collect_let_const_declarations(stmts: &[Statement]) -> Vec<(String, VarKind)> {
    let mut decls = Vec::new();
    collect_let_const_recursive(stmts, &mut decls);
    decls.sort_by(|a, b| a.0.cmp(&b.0));
    decls.dedup_by(|a, b| a.0 == b.0);
    decls
}

pub(crate) fn collect_let_const_recursive(stmts: &[Statement], decls: &mut Vec<(String, VarKind)>) {
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
            Statement::SequenceDecls(inner) => {
                collect_let_const_recursive(inner, decls);
            }
            Statement::Block(inner) => {
                collect_let_const_recursive(inner, decls);
            }
            _ => {}
        }
    }
}

pub(crate) fn predeclare_var(stmts: &[Statement], env: &mut Environment) {
    let names = collect_var_names(stmts);
    for name in names {
        env.declare_var(name, VarKind::Var);
    }
}

pub(crate) fn predeclare_let_const(stmts: &[Statement], env: &mut Environment) {
    let decls = collect_let_const_declarations(stmts);
    for (name, kind) in decls {
        // Skip if already declared in any outer scope (let/const cannot be shadowed
        // by redeclaring in an inner block - they share the same binding)
        if !env.has(&name) {
            env.declare_var(name, kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_depth() {
        reset_depth();
    }

    #[test]
    fn test_has_legacy_octal() {
        assert!(has_legacy_octal("01"), "01 is legacy octal");
        assert!(has_legacy_octal("07"), "07 is legacy octal");
        assert!(!has_legacy_octal("0x1"), "0x1 is hex, not octal");
        assert!(!has_legacy_octal("0X1"), "0X1 is hex, not octal");
        assert!(!has_legacy_octal("0b1"), "0b1 is binary, not octal");
        assert!(!has_legacy_octal("0B1"), "0B1 is binary, not octal");
        assert!(!has_legacy_octal("0o1"), "0o1 is octal, not legacy");
        assert!(!has_legacy_octal("0O1"), "0O1 is octal, not legacy");
        assert!(!has_legacy_octal("0n"), "0n is bigint, not octal");
        assert!(has_legacy_octal("a = 01;"), "a = 01; has octal");
        assert!(
            has_legacy_octal("\"use strict\";\na = 01;"),
            "with strict prefix"
        );
        assert!(
            !has_legacy_octal("\"use strict\";\nvar threw = false;"),
            "strict source, no octal"
        );
        // Copyright year like "// (C) 2015" must NOT be flagged
        assert!(
            !has_legacy_octal("// Copyright (C) 2015 the V8 project authors."),
            "copyright year 2015 is not an octal"
        );
        assert!(
            !has_legacy_octal("// Copyright (C) 2016 the V8 project authors."),
            "copyright year 2016 is not an octal"
        );
        // Numbers that contain 01/07 embedded are not octals
        assert!(!has_legacy_octal("var x = 2015;"), "2015 is not octal");
        assert!(!has_legacy_octal("var x = 1007;"), "1007 is not octal");
        assert!(!has_legacy_octal("var x = 1.07;"), "1.07 is not octal");
        // 0.07 has 0 preceded by . → not an octal
        assert!(!has_legacy_octal("var x = 0.07;"), "0.07 is not octal");
        // Octal inside string literals (like assert.sameValue(..., "0001")) → not an octal
        assert!(
            !has_legacy_octal(r#"assert.sameValue(decimalToHexString(1), "0001");"#),
            "\"0001\" string literal is not octal"
        );
        // All harness strings with embedded 0+N patterns: "0229", "012", "0123456789ABCDEF" etc.
        assert!(
            !has_legacy_octal(r#"var hex = "0123456789ABCDEF";"#),
            "hex string is not octal"
        );
        // "%01" inside string literal — must NOT be flagged (decimalToHexString.js test)
        assert!(
            !has_legacy_octal(r#"assert.sameValue(decimalToPercentHexString(1), "%01");"#),
            "\"%01\" string literal is not octal"
        );
        // Full decimalToHexString.js test source (sloppy + strict) — must NOT be flagged
        assert!(
            !has_legacy_octal(
                r#""use strict";
function decimalToHexString(n) {
  var hex = "0123456789ABCDEF";
  return "%" + hex[(n >> 4) & 0xf] + hex[n & 0xf];
}
assert.sameValue(decimalToHexString(1), "0001");
assert.sameValue(decimalToPercentHexString(1), "%01");"#
            ),
            "decimalToHexString.js strict source is not octal"
        );
        // Template literal with embedded 0 — not an octal
        assert!(
            !has_legacy_octal(r#"var s = `prefix 01 suffix`;"#),
            "template literal with 01 is not octal"
        );
        // Block comment with embedded 0 — not an octal
        assert!(
            !has_legacy_octal(r#"/* octal: 01 in comment */"#),
            "block comment with 01 is not octal"
        );
        // Regex literal with \u02C1 — the '0' in \u02C1 must NOT be flagged as octal
        assert!(
            !has_legacy_octal(
                r#""use strict";
var UnicodeIDStart = /[a-zA-Z\xF6\xF8-\u02C1]/u;"#
            ),
            "regex literal with \\u02C1 is not octal"
        );
        // [native code] matcher regex — must NOT be flagged
        assert!(
            !has_legacy_octal(
                r#""use strict";
var re = /\[native code\]/"#
            ),
            "regex literal [native code] is not octal"
        );
        // UTF-8 multi-byte sequences: \u{11A01} encodes as F0 91 A8 81.
        // The byte 0xA8 (high bit set) precedes 0x81; neither is a standalone '0'.
        // The fix: skip '0' when preceded by a UTF-8 continuation byte.
        assert!(
            !has_legacy_octal("var _\u{0AFA}\u{0AFB}\u{0AFC};"),
            "UTF-8 multi-byte chars should not trigger octal false positive"
        );
        // Regex with char class containing 0+N — must NOT be flagged
        assert!(
            !has_legacy_octal(r#"var re = /[01]/;"#),
            "regex literal with char class 01 is not octal"
        );
        // Non-octal decimal integers (08, 09, 018, etc.) — 8 and 9 are not octal digits
        assert!(
            !has_legacy_octal("08"),
            "08 is not octal (8 is not an octal digit)"
        );
        assert!(
            !has_legacy_octal("09"),
            "09 is not octal (9 is not an octal digit)"
        );
        assert!(
            !has_legacy_octal("018"),
            "018 is not octal (8 is not an octal digit)"
        );
        assert!(
            !has_legacy_octal("019"),
            "019 is not octal (9 is not an octal digit)"
        );
        assert!(
            !has_legacy_octal("assert.sameValue(08, 8);"),
            "08 in assert is not octal"
        );
        assert!(
            !has_legacy_octal("assert.sameValue(018, 18);"),
            "018 in assert is not octal"
        );
        // Numeric separators in decimals (00_01, 10.00_01e2) — underscores are not octal
        assert!(
            !has_legacy_octal("var x = 00_01;"),
            "00_01 with separator is not octal"
        );
        assert!(
            !has_legacy_octal("assert.sameValue(10.00_01e2, 10.0001e2);"),
            "10.00_01e2 is not octal"
        );
        // Actual octal numbers in code must still be detected
        assert!(has_legacy_octal("var x = 01;"), "01 in code is octal");
        assert!(
            has_legacy_octal("assert.sameValue(01, 1);"),
            "01 in assert is octal"
        );
    }
}
