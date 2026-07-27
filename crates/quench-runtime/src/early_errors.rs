//! Early error checking for test262 parse-phase errors.
//!
//! Called after the OXC parser produces an AST (which it accepts as valid JS)
//! but before lowering, so test262 tests with `negative: phase: parse` get
//! the SyntaxError they expect.
//!
//! Each check is backed by a failing unit test and covers exactly the early
//! error the spec defines. No speculative checks.

use oxc::ast::ast::{self, ForStatementLeft};
use oxc::ast_visit::Visit;
use crate::value::JsError;

/// Check all early errors on the OXC program before lowering.
/// Called from `parser.rs` after parsing.
pub fn check_early_errors(program: &ast::Program) -> Result<(), JsError> {
    let strict = program
        .directives
        .iter()
        .any(|d| d.expression.value == "use strict")
        || crate::interpreter::is_strict_mode();
    for stmt in &program.body {
        check_stmt(stmt, strict)?;
        // Also walk functions in statement position for parameter errors
        check_fn_params_in_stmt(stmt)?;
    }
    Ok(())
}

fn check_stmt(stmt: &ast::Statement, strict: bool) -> Result<(), JsError> {
    match stmt {
        ast::Statement::ForOfStatement(for_of) => {
            check_for_of_declaration_errors(for_of, strict)?;
            check_for_of_body_errors(for_of)?;
            check_for_of_binding_conflicts(for_of)?;
        }
        ast::Statement::WhileStatement(while_stmt) => {
            // Function declaration in while body is a SyntaxError (§14.1.0)
            check_no_fn_decl_in_stmt(&while_stmt.body)?;
            // Walk the body for nested function parameter errors
            walk_inner_statements_for_fn_params(&while_stmt.body, strict)?;
        }
        ast::Statement::DoWhileStatement(do_while) => {
            check_no_fn_decl_in_stmt(&do_while.body)?;
            walk_inner_statements_for_fn_params(&do_while.body, strict)?;
        }
        ast::Statement::ForStatement(for_stmt) => {
            check_no_fn_decl_in_stmt(&for_stmt.body)?;
            walk_inner_statements_for_fn_params(&for_stmt.body, strict)?;
        }
        ast::Statement::ForInStatement(for_in) => {
            check_no_fn_decl_in_stmt(&for_in.body)?;
            walk_inner_statements_for_fn_params(&for_in.body, strict)?;
        }
        ast::Statement::IfStatement(if_stmt) => {
            check_no_fn_decl_in_stmt(&if_stmt.consequent)?;
            if let Some(alt) = &if_stmt.alternate {
                check_no_fn_decl_in_stmt(alt)?;
            }
            walk_inner_statements_for_fn_params(&if_stmt.consequent, strict)?;
            if let Some(alt) = &if_stmt.alternate {
                walk_inner_statements_for_fn_params(alt, strict)?;
            }
        }
        ast::Statement::LabeledStatement(labeled) => {
            if strict && is_named_function(&labeled.body) {
                return Err(JsError(
                    "SyntaxError: Labeled function declaration in strict mode is not allowed".into(),
                ));
            }
            // `yield` as label in strict mode: SyntaxError (§13.1.1)
            if strict && labeled.label.name == "yield" {
                return Err(JsError(
                    "SyntaxError: Unexpected strict mode reserved word 'yield'".into(),
                ));
            }
        }
        ast::Statement::ExpressionStatement(expr) => {
            check_expr_for_fn_params(&expr.expression)?;
        }
        ast::Statement::VariableDeclaration(var_decl) => {
            for d in &var_decl.declarations {
                if let Some(init) = &d.init {
                    check_expr_for_fn_params(init)?;
                }
            }
        }
        ast::Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                check_expr_for_fn_params(arg)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Walk statements inside a loop/if body for function parameter early errors.
fn walk_inner_statements_for_fn_params(stmt: &ast::Statement, strict: bool) -> Result<(), JsError> {
    match stmt {
        ast::Statement::BlockStatement(block) => {
            for s in &block.body {
                check_stmt(s, strict)?;
            }
        }
        ast::Statement::FunctionDeclaration(func) => {
            if let Some(body) = &func.body {
                check_fn_params(&func.params, body)?;
            }
        }
        ast::Statement::ExpressionStatement(expr) => {
            check_expr_for_fn_params(&expr.expression)?;
        }
        ast::Statement::LabeledStatement(labeled) => {
            walk_inner_statements_for_fn_params(&labeled.body, strict)?;
        }
        _ => {}
    }
    Ok(())
}

/// Check that a statement is not a FunctionDeclaration in a disallowed position.
fn check_no_fn_decl_in_stmt(stmt: &ast::Statement) -> Result<(), JsError> {
    match stmt {
        // `while (false) function f() {}` — SyntaxError (§14.1.0)
        ast::Statement::FunctionDeclaration(_) => {
            return Err(JsError(
                "SyntaxError: Function declaration not allowed in statement position".into(),
            ));
        }
        // Check inside labeled statements (non-strict: Annex B.3.2 allows it at top level)
        ast::Statement::LabeledStatement(labeled) => {
            check_no_fn_decl_in_stmt(&labeled.body)
        }
        _ => Ok(()),
    }
}

/// Check if a statement is a (named) function declaration.
fn is_named_function(stmt: &ast::Statement<'_>) -> bool {
    matches!(stmt, ast::Statement::FunctionDeclaration(_))
}

fn check_expr_for_fn_params(expr: &ast::Expression) -> Result<(), JsError> {
    match expr {
        ast::Expression::ArrowFunctionExpression(arrow) => {
            check_fn_params(&arrow.params, &arrow.body)?;
        }
        ast::Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                check_fn_params(&func.params, body)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_fn_params_in_stmt(stmt: &ast::Statement) -> Result<(), JsError> {
    if let ast::Statement::FunctionDeclaration(func) = stmt {
        if let Some(body) = &func.body {
            check_fn_params(&func.params, body)?;
        }
    }
    Ok(())
}

/// Check function parameter early errors:
/// 1. Rest parameter cannot have initializer (e.g. `(...x = []) => {}`)
/// 2. If body is strict, array/object destructuring params are SyntaxError
///
/// Note: Duplicate parameter names with defaults is checked by `check_fn_dup_params`.
fn check_fn_params(
    params: &ast::FormalParameters,
    body: &ast::FunctionBody,
) -> Result<(), JsError> {
    check_rest_param_no_init(params)?;
    check_body_strict_with_destructuring(params, body)?;
    check_dup_params_with_defaults(params)?;
    // Check nested rest elements in parameter binding patterns
    for param in &params.items {
        check_rest_no_init(&param.pattern)?;
    }
    Ok(())
}

/// Rest parameter with initializer is SyntaxError.
/// ES2025 §13.3.3: `BindingRestElement : ... BindingIdentifier` cannot have Initializer.
fn check_rest_param_no_init(params: &ast::FormalParameters) -> Result<(), JsError> {
    if let Some(rest) = &params.rest {
        if matches!(rest.rest.argument, ast::BindingPattern::AssignmentPattern(_)) {
            return Err(JsError(
                "SyntaxError: Rest parameter may not have an initializer".into(),
            ));
        }
    }
    Ok(())
}

/// In strict mode, array/object destructuring in parameters is SyntaxError.
/// ES2025 §14.1.2: It is a Syntax Error if the function body is strict and
/// any formal parameter contains a BindingPattern.
fn check_body_strict_with_destructuring(
    params: &ast::FormalParameters,
    body: &ast::FunctionBody,
) -> Result<(), JsError> {
    let is_strict = body
        .directives
        .iter()
        .any(|d| d.expression.value == "use strict");
    if !is_strict {
        return Ok(());
    }
    for param in &params.items {
        if has_destructuring_pattern(&param.pattern) {
            return Err(JsError(
                "SyntaxError: Destructuring parameter not allowed in strict mode function".into(),
            ));
        }
    }
    if let Some(rest) = &params.rest {
        if has_destructuring_pattern(&rest.rest.argument) {
            return Err(JsError(
                "SyntaxError: Destructuring parameter not allowed in strict mode function".into(),
            ));
        }
    }
    Ok(())
}

fn has_destructuring_pattern(pattern: &ast::BindingPattern) -> bool {
    match pattern {
        ast::BindingPattern::ObjectPattern(_) | ast::BindingPattern::ArrayPattern(_) => true,
        ast::BindingPattern::AssignmentPattern(assign) => has_destructuring_pattern(&assign.left),
        _ => false,
    }
}

/// Duplicate parameter names are SyntaxError when parameters have default values.
/// ES2025 §14.1.2: It is a Syntax Error if BoundNames of FormalParameters
/// contains any duplicate entries. (Note: duplicates are allowed without
/// defaults in non-strict mode.)
fn check_dup_params_with_defaults(params: &ast::FormalParameters) -> Result<(), JsError> {
    let has_defaults = params.items.iter().any(|p| {
        matches!(p.pattern, ast::BindingPattern::AssignmentPattern(_))
    });
    if !has_defaults {
        return Ok(());
    }
    let mut seen = std::collections::HashSet::new();
    for param in &params.items {
        collect_binding_names(&param.pattern, &mut |name| {
            if !seen.insert(name.to_string()) {
                // Found duplicate
            }
        });
    }
    // Check for duplicates: compare against the initial list
    let mut seen_names = std::collections::HashSet::new();
    let mut dup_found = false;
    let mut dup_name = String::new();
    for param in &params.items {
        collect_binding_names(&param.pattern, &mut |name| {
            let name_s = name.to_string();
            if !seen_names.insert(name_s.clone()) {
                dup_found = true;
                dup_name = name_s;
            }
        });
    }
    if dup_found {
        return Err(JsError(format!(
            "SyntaxError: Duplicate parameter name '{}' not allowed in this context",
            dup_name
        )));
    }
    Ok(())
}

fn collect_binding_names(pattern: &ast::BindingPattern, f: &mut impl FnMut(&str)) {
    match pattern {
        ast::BindingPattern::BindingIdentifier(id) => f(&id.name),
        ast::BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_names(&prop.value, f);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_names(&rest.argument, f);
            }
        }
        ast::BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_names(elem, f);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_names(&rest.argument, f);
            }
        }
        ast::BindingPattern::AssignmentPattern(assign) => {
            collect_binding_names(&assign.left, f);
        }
    }
}

/// Renamed function kept for backward compatibility (parser.rs still calls it).
pub fn check_for_of_early_errors(program: &ast::Program) -> Result<(), JsError> {
    check_early_errors(program)
}

/// Check: ForDeclaration initializers and rest element initializers.
/// ES2025 §13.7.5.1:
///   `for (ForDeclaration of AssignmentExpression) Statement`
///   — SyntaxError if ForDeclaration has an Initializer.
///   `for (var ForBinding of AssignmentExpression) Statement`
///   — SyntaxError if ForBinding has an Initializer (Annex B allows init ONLY
///     for for-in, not for-of).
///
/// Also: BindingRestElement/BindingRestProperty cannot have initializer.
/// Also: BoundNames of ForDeclaration cannot contain "let".
fn check_for_of_declaration_errors(for_of: &ast::ForOfStatement, strict: bool) -> Result<(), JsError> {
    match &for_of.left {
        ForStatementLeft::VariableDeclaration(var_decl) => {
            // No declaration in for-of may have an initializer (even var).
            for decl in &var_decl.declarations {
                if decl.init.is_some() {
                    return Err(JsError(
                        "SyntaxError: for-of ForDeclaration may not have an initializer".into(),
                    ));
                }
            }
            // Rest element in binding pattern: no initializer (e.g. [...x = []] = [])
            for decl in &var_decl.declarations {
                check_rest_no_init(&decl.id)?;
            }
            // BoundNames of ForDeclaration cannot contain "let" (§13.7.5.1).
            // This restriction applies only to LetOrConst (ForDeclaration),
            // not to var declarations.
            if !var_decl.kind.is_var() {
                let names = collect_bound_names(var_decl);
                for name in &names {
                    if name == "let" {
                        return Err(JsError(
                            "SyntaxError: BoundNames of ForDeclaration cannot contain 'let'".into(),
                        ));
                    }
                }
            }
            // In strict mode, eval and arguments cannot be binding identifiers
            // in destructuring patterns (§13.1.1 / §14.1.2).
            if strict {
                for decl in &var_decl.declarations {
                    check_strict_binding(&decl.id)?;
                }
            }
        }
        // For destructuring assignment targets in for-of (e.g. `for ({ eval } of ...)`),
        // check binding identifiers in strict mode.
        _ => {
            if strict {
                check_for_of_lhs_strict_binding(&for_of.left)?;
            }
        }
    }
    Ok(())
}

/// Check strict-mode binding identifiers in a ForStatementLeft that is not a
/// VariableDeclaration (e.g., ObjectAssignmentTarget, ArrayAssignmentTarget).
fn check_for_of_lhs_strict_binding(left: &ast::ForStatementLeft) -> Result<(), JsError> {
    match left {
        ForStatementLeft::AssignmentTargetIdentifier(ident) => {
            if ident.name == "eval" || ident.name == "arguments" {
                return Err(JsError(format!(
                    "SyntaxError: Unexpected strict mode reserved word '{}'",
                    ident.name
                )));
            }
        }
        ForStatementLeft::ObjectAssignmentTarget(obj) => {
            for prop in &obj.properties {
                match prop {
                    ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
                        if id.binding.name == "eval" || id.binding.name == "arguments" {
                            return Err(JsError(format!(
                                "SyntaxError: Unexpected strict mode reserved word '{}'",
                                id.binding.name
                            )));
                        }
                    }
                    ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                        check_assignment_target_inner(&p.binding)?;
                    }
                }
            }
        }
        ForStatementLeft::ArrayAssignmentTarget(arr) => {
            for elem in arr.elements.iter().flatten() {
                check_assignment_target_inner(elem)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_assignment_target_inner(target: &ast::AssignmentTargetMaybeDefault) -> Result<(), JsError> {
    if let Some(assignment_target) = target.as_assignment_target() {
        if let ast::AssignmentTarget::AssignmentTargetIdentifier(ident) = assignment_target {
            if ident.name == "eval" || ident.name == "arguments" {
                return Err(JsError(format!(
                    "SyntaxError: Unexpected strict mode reserved word '{}'",
                    ident.name
                )));
            }
        }
    }
    Ok(())
}

/// Walk a BindingPattern looking for rest elements with default values.
/// The spec says BindingRestElement / BindingRestProperty cannot have an
/// Initializer. In OXC's AST a rest-with-default appears as a
/// BindingRestElement whose .argument.kind is AssignmentPattern.
fn check_rest_no_init(pattern: &ast::BindingPattern) -> Result<(), JsError> {
    match pattern {
        ast::BindingPattern::ArrayPattern(arr) => {
            if let Some(rest) = &arr.rest {
                if matches!(rest.argument, ast::BindingPattern::AssignmentPattern(_)) {
                    return Err(JsError(
                        "SyntaxError: rest element may not have an initializer".into(),
                    ));
                }
            }
            // Check elements
            for elem in arr.elements.iter().flatten() {
                check_rest_no_init(elem)?;
            }
        }
        ast::BindingPattern::ObjectPattern(obj) => {
            if let Some(rest) = &obj.rest {
                if matches!(rest.argument, ast::BindingPattern::AssignmentPattern(_)) {
                    return Err(JsError(
                        "SyntaxError: rest element may not have an initializer".into(),
                    ));
                }
            }
            for prop in &obj.properties {
                check_rest_no_init(&prop.value)?;
            }
        }
        ast::BindingPattern::AssignmentPattern(assign) => {
            check_rest_no_init(&assign.left)?;
        }
        _ => {}
    }
    Ok(())
}

/// In strict mode, `eval` and `arguments` cannot appear as binding identifiers
/// in a destructuring pattern (§13.1.1).
fn check_strict_binding(pattern: &ast::BindingPattern) -> Result<(), JsError> {
    match pattern {
        ast::BindingPattern::BindingIdentifier(id) => {
            if id.name == "eval" || id.name == "arguments" {
                return Err(JsError(format!(
                    "SyntaxError: Unexpected strict mode reserved word '{}'",
                    id.name
                )));
            }
        }
        ast::BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                check_strict_binding(&prop.value)?;
            }
            if let Some(rest) = &obj.rest {
                check_strict_binding(&rest.argument)?;
            }
        }
        ast::BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                check_strict_binding(elem)?;
            }
            if let Some(rest) = &arr.rest {
                check_strict_binding(&rest.argument)?;
            }
        }
        ast::BindingPattern::AssignmentPattern(assign) => {
            check_strict_binding(&assign.left)?;
        }
    }
    Ok(())
}

/// Check: function declarations / labelled function statements in for-of body.
/// ES2025 §13.7.5.1: SyntaxError if IsLabelledFunction(Statement) is false.
/// Unlabelled FunctionDeclaration always has IsLabelledFunction = false.
/// Annex B.3.2: labelled function in for-of/for-in is always SyntaxError.
fn check_for_of_body_errors(for_of: &ast::ForOfStatement) -> Result<(), JsError> {
    check_body_for_function_decl(&for_of.body)
}

fn check_body_for_function_decl(stmt: &ast::Statement) -> Result<(), JsError> {
    match stmt {
        // `for (var x of []) function f() {}` — IsLabelledFunction false
        ast::Statement::FunctionDeclaration(_) => {
            return Err(JsError(
                "SyntaxError: Function declaration in for-of statement body is not allowed".into(),
            ));
        }
        // `for (const x of []) label: function f() {}` — labelled function
        // Always error in for-of per Annex B.3.2
        // Also handles nested labels: label1: label2: function f() {}
        ast::Statement::LabeledStatement(labeled) => {
            if is_any_label_wrapping_fn(&labeled.body) {
                return Err(JsError(
                    "SyntaxError: Labelled function declaration in for-of statement body is not allowed"
                        .into(),
                ));
            }
        }
        // Check inside blocks
        ast::Statement::BlockStatement(block) => {
            for s in &block.body {
                if matches!(s, ast::Statement::FunctionDeclaration(_)) {
                    return Err(JsError(
                        "SyntaxError: Function declaration in for-of statement body is not allowed"
                            .into(),
                    ));
                }
                if let ast::Statement::LabeledStatement(labeled) = s {
                    if is_any_label_wrapping_fn(&labeled.body) {
                        return Err(JsError(
                            "SyntaxError: Labelled function declaration in for-of statement body is not allowed"
                                .into(),
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Walk through nested LabeledStatements to check if any wraps a FunctionDeclaration.
fn is_any_label_wrapping_fn(stmt: &ast::Statement) -> bool {
    match stmt {
        ast::Statement::FunctionDeclaration(_) => true,
        ast::Statement::LabeledStatement(labeled) => {
            is_any_label_wrapping_fn(&labeled.body)
        }
        _ => false,
    }
}

/// Check: BoundNames of ForDeclaration vs VarDeclaredNames of Statement.
/// ES2025 §13.7.5.1:
///   SyntaxError if BoundNames of ForDeclaration overlaps VarDeclaredNames of Statement.
///   SyntaxError if BoundNames has duplicates.
fn check_for_of_binding_conflicts(for_of: &ast::ForOfStatement) -> Result<(), JsError> {
    let (bound_names, is_var) = match &for_of.left {
        ForStatementLeft::VariableDeclaration(var_decl) => {
            let names = collect_bound_names(var_decl);
            (names, var_decl.kind.is_var())
        }
        _ => return Ok(()),
    };

    // These checks apply only to ForDeclaration (let/const), not var (§13.7.5.1).
    // var declarations allow duplicates and body redeclarations.
    if !is_var {
        // Check duplicates in ForDeclaration
        let mut seen = std::collections::HashSet::new();
        for name in &bound_names {
            if !seen.insert(name.clone()) {
                return Err(JsError(format!(
                    "SyntaxError: Duplicate binding '{}' in for-of declaration",
                    name
                )));
            }
        }

        // Check overlap with var-declared names in body
        let var_names = collect_var_declared(&for_of.body);
        for name in &bound_names {
            if var_names.contains(name) {
                return Err(JsError(format!(
                    "SyntaxError: '{}' already declared in for-of head but also in statement body",
                    name
                )));
            }
        }
    }

    Ok(())
}

/// Collect binding names from a VariableDeclaration (flattening destructuring).
fn collect_bound_names(var_decl: &ast::VariableDeclaration) -> Vec<String> {
    let mut names = Vec::new();
    for decl in &var_decl.declarations {
        collect_names_from_pattern(&decl.id, &mut names);
    }
    names
}

fn collect_names_from_pattern(pattern: &ast::BindingPattern, names: &mut Vec<String>) {
    match pattern {
        ast::BindingPattern::BindingIdentifier(id) => {
            names.push(id.name.to_string());
        }
        ast::BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_names_from_pattern(&prop.value, names);
            }
            if let Some(rest) = &obj.rest {
                collect_names_from_pattern(&rest.argument, names);
            }
        }
        ast::BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_names_from_pattern(elem, names);
            }
            if let Some(rest) = &arr.rest {
                collect_names_from_pattern(&rest.argument, names);
            }
        }
        ast::BindingPattern::AssignmentPattern(assign) => {
            collect_names_from_pattern(&assign.left, names);
        }
    }
}

/// Collect var-declared names from a statement (recursively through blocks etc).
fn collect_var_declared(stmt: &ast::Statement) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    collect_var_names(stmt, &mut names);
    names
}

fn collect_var_names(stmt: &ast::Statement, names: &mut std::collections::HashSet<String>) {
    match stmt {
        ast::Statement::VariableDeclaration(decl) => {
            if decl.kind.is_var() {
                for d in &decl.declarations {
                    collect_idents_from_pattern(&d.id, names);
                }
            }
        }
        ast::Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_var_names(s, names);
            }
        }
        ast::Statement::LabeledStatement(labeled) => {
            // labeled.body is Statement<'a> directly (not Box)
            collect_var_names(&labeled.body, names);
        }
        ast::Statement::IfStatement(if_stmt) => {
            collect_var_names(&if_stmt.consequent, names);
            if let Some(alt) = &if_stmt.alternate {
                collect_var_names(alt, names);
            }
        }
        ast::Statement::ForStatement(for_stmt) => {
            collect_var_names(&for_stmt.body, names);
        }
        ast::Statement::ForInStatement(for_in) => {
            collect_var_names(&for_in.body, names);
        }
        ast::Statement::ForOfStatement(for_of) => {
            collect_var_names(&for_of.body, names);
        }
        ast::Statement::WhileStatement(while_stmt) => {
            collect_var_names(&while_stmt.body, names);
        }
        ast::Statement::DoWhileStatement(do_while) => {
            collect_var_names(&do_while.body, names);
        }
        ast::Statement::SwitchStatement(switch_stmt) => {
            for case in &switch_stmt.cases {
                for s in &case.consequent {
                    collect_var_names(s, names);
                }
            }
        }
        ast::Statement::TryStatement(try_stmt) => {
            // BlockStatement has a .body: Vec<Statement>
            for s in &try_stmt.block.body {
                collect_var_names(s, names);
            }
            if let Some(handler) = &try_stmt.handler {
                for s in &handler.body.body {
                    collect_var_names(s, names);
                }
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                for s in &finalizer.body {
                    collect_var_names(s, names);
                }
            }
        }
        _ => {}
    }
}

fn collect_idents_from_pattern(pattern: &ast::BindingPattern, names: &mut std::collections::HashSet<String>) {
    match pattern {
        ast::BindingPattern::BindingIdentifier(id) => {
            names.insert(id.name.to_string());
        }
        ast::BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_idents_from_pattern(&prop.value, names);
            }
            if let Some(rest) = &obj.rest {
                collect_idents_from_pattern(&rest.argument, names);
            }
        }
        ast::BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_idents_from_pattern(elem, names);
            }
            if let Some(rest) = &arr.rest {
                collect_idents_from_pattern(&rest.argument, names);
            }
        }
        ast::BindingPattern::AssignmentPattern(assign) => {
            collect_idents_from_pattern(&assign.left, names);
        }
    }
}

/// Walk iteration context for break/continue validation using OXC's Visit trait.
/// Check if a statement is an iteration statement suitable for continue/break labels.
fn iteration_stmt_kind(stmt: &ast::Statement) -> bool {
    matches!(
        stmt,
        ast::Statement::WhileStatement(_)
            | ast::Statement::DoWhileStatement(_)
            | ast::Statement::ForStatement(_)
            | ast::Statement::ForInStatement(_)
            | ast::Statement::ForOfStatement(_)
    )
}

pub fn check_break_continue_errors(program: &ast::Program) -> Result<(), JsError> {
    let mut checker = BreakContinueChecker {
        for_depth: 0,
        switch_depth: 0,
        iter_labels: Vec::new(),
        all_labels: Vec::new(),
        error: None,
    };
    checker.visit_program(program);
    match checker.error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

struct BreakContinueChecker {
    for_depth: usize,
    switch_depth: usize,
    /// Labels that refer to iteration statements (for continue/break with label)
    iter_labels: Vec<String>,
    /// All labels currently in scope (for tracking which are iteration labels)
    all_labels: Vec<(String, bool)>, // (name, is_iteration)
    error: Option<JsError>,
}

impl<'a> Visit<'a> for BreakContinueChecker {
    fn visit_break_statement(&mut self, it: &oxc::ast::ast::BreakStatement) {
        if self.error.is_some() {
            return;
        }
        if let Some(label) = &it.label {
            // Labeled break is valid if the label refers to any enclosing statement
            // (iteration or switch). If the label isn't in our scope, it's an error.
            if !self.all_labels.iter().any(|(n, _)| n == &label.name.as_str()) {
                self.error = Some(JsError(
                    "SyntaxError: Undefined label '".to_string() + &label.name + "'",
                ));
            }
        } else if self.for_depth == 0 && self.switch_depth == 0 {
            self.error = Some(JsError("SyntaxError: Illegal break statement".into()));
        }
    }

    fn visit_continue_statement(&mut self, it: &oxc::ast::ast::ContinueStatement) {
        if self.error.is_some() {
            return;
        }
        if let Some(label) = &it.label {
            // Labeled continue is only valid when the label refers to an iteration statement
            if !self.iter_labels.contains(&label.name.as_str().to_string()) {
                // Check if it's a non-iteration label (exists but not iteration)
                let is_known_non_iter = self.all_labels.iter().any(|(n, is_iter)| {
                    n == &label.name.as_str() && !is_iter
                });
                if is_known_non_iter || !self.all_labels.iter().any(|(n, _)| n == &label.name.as_str()) {
                    self.error = Some(JsError(
                        "SyntaxError: Undefined label '".to_string() + &label.name + "'",
                    ));
                }
            }
        } else if self.for_depth == 0 {
            self.error = Some(JsError("SyntaxError: Illegal continue statement".into()));
        }
    }

    fn visit_labeled_statement(&mut self, it: &oxc::ast::ast::LabeledStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        let label_name = it.label.name.as_str().to_string();
        // Check if the labeled statement body is an iteration statement
        let is_iter = iteration_stmt_kind(&it.body);
        self.all_labels.push((label_name.clone(), is_iter));
        if is_iter {
            self.iter_labels.push(label_name);
        }
        // Visit the body
        self.visit_statement(&it.body);
        // Pop labels
        self.all_labels.pop();
        if is_iter {
            self.iter_labels.pop();
        }
    }

    fn visit_switch_statement(&mut self, it: &oxc::ast::ast::SwitchStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        self.switch_depth += 1;
        for case in &it.cases {
            for stmt in &case.consequent {
                self.visit_statement(stmt);
            }
        }
        self.switch_depth -= 1;
    }

    fn visit_while_statement(&mut self, it: &oxc::ast::ast::WhileStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        self.for_depth += 1;
        self.visit_statement(&it.body);
        self.for_depth -= 1;
    }

    fn visit_do_while_statement(&mut self, it: &oxc::ast::ast::DoWhileStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        self.for_depth += 1;
        self.visit_statement(&it.body);
        self.for_depth -= 1;
    }

    fn visit_for_statement(&mut self, it: &oxc::ast::ast::ForStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        self.for_depth += 1;
        oxc::ast_visit::Visit::visit_statement(self, &it.body);
        self.for_depth -= 1;
    }

    fn visit_for_in_statement(&mut self, it: &oxc::ast::ast::ForInStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        self.for_depth += 1;
        oxc::ast_visit::Visit::visit_statement(self, &it.body);
        self.for_depth -= 1;
    }

    fn visit_for_of_statement(&mut self, it: &oxc::ast::ast::ForOfStatement<'a>) {
        if self.error.is_some() {
            return;
        }
        self.for_depth += 1;
        oxc::ast_visit::Visit::visit_statement(self, &it.body);
        self.for_depth -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    fn test_source(source: &str) -> ast::Program {
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC parse errors: {:?}", ret.diagnostics);
        // Can't return ret.program because it borrows allocator.
        // Instead just run checks inside.
        unimplemented!()
    }

    #[test]
    fn for_of_const_init_is_error() {
        let s = "for (const x = 1 of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err(), "Expected SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_let_init_is_error() {
        let s = "for (let x = 1 of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_var_init_is_error() {
        let s = "for (var x = 1 of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err(), "var init should be SyntaxError in for-of: {:?}", result);
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_no_init_is_ok() {
        let s = "for (const x of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok(), "no init should be ok: {:?}", result);
    }

    #[test]
    fn for_of_rest_array_init_is_error() {
        let s = "for (const [...[x] = []] of [[]]) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err(), "rest init should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_fn_decl_in_body_is_error() {
        let s = "for (var x of []) function f() {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_fn_decl_in_block_body_is_error() {
        let s = "for (var x of []) { function f() {} }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_labelled_fn_is_error() {
        let s = "for (const x of []) label1: label2: function f() {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_bound_name_conflict_with_var() {
        let s = "for (const x of []) { var x; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_no_conflict_with_let() {
        let s = "for (const x of []) { let y; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok(), "let y should not conflict: {:?}", result);
    }

    #[test]
    fn for_of_valid_for_of_is_ok() {
        let s = "for (const x of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok());
    }

    #[test]
    fn for_of_let_as_binding_name_is_error() {
        let s = "for (const let of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err(), "let as bound name should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn for_of_var_let_allowed() {
        // var let is valid (let is not a reserved word in sloppy mode)
        let s = "for (var let of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok(), "var let should be allowed: {:?}", result);
    }

    #[test]
    fn for_of_var_dup_allowed() {
        // var [x, x] duplicates are allowed (last wins)
        let s = "for (var [x, x] of [[1, 2]]) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok(), "var dup should be allowed: {:?}", result);
    }

    #[test]
    fn for_of_var_body_redeclaration_allowed() {
        // var x in body can redeclare var x in head
        let s = "for (var x of []) { var x; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok(), "var body redeclaration should be allowed: {:?}", result);
    }

    #[test]
    fn for_of_valid_member_lhs() {
        let s = "for (obj.x of []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok());
    }

    #[test]
    fn for_of_eval_destructuring_in_strict_is_error() {
        // `"use strict"; for ({ eval = 0 } of [{}]) ;` is a SyntaxError
        // because `eval` is not a valid binding in strict mode.
        let s = "\"use strict\"; for ({ eval = 0 } of [{}]) ;";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_err(), "eval in strict destructuring should be SyntaxError");
    }

    #[test]
    fn for_of_eval_destructuring_in_sloppy_is_ok() {
        // In sloppy mode (no "use strict"), `for ({ eval = 0 } of [{}]) ;` is fine.
        let s = "for ({ eval = 0 } of [{}]) ;";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_for_of_early_errors(&ret.program);
        assert!(result.is_ok(), "eval in sloppy destructuring should be ok");
    }

    // ===== Function parameter early errors =====

    #[test]
    fn arrow_rest_param_with_default_is_error() {
        let s = "(...x = []) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "rest+default should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn fn_rest_param_with_default_is_error() {
        let s = "function f(...x = []) {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "rest+default should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn arrow_dup_params_with_defaults_is_error() {
        let s = "(x = 1, x = 2) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "duplicate params+default should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn arrow_array_destr_in_strict_body_is_error() {
        let s = "([x]) => { 'use strict'; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "array destr in strict body should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn arrow_obj_destr_in_strict_body_is_error() {
        let s = "({x}) => { 'use strict'; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "obj destr in strict body should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn arrow_plain_params_in_strict_body_is_ok() {
        let s = "(x, y) => { 'use strict'; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_early_errors(&ret.program);
        assert!(result.is_ok(), "plain params in strict body should be ok: {:?}", result);
    }

    #[test]
    fn arrow_dstr_rest_array_with_init_is_error() {
        let s = "([...x = []]) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "nested rest init should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn arrow_dstr_rest_with_obj_pattern_init_is_error() {
        // Rest element with object pattern and default: [...{x} = []] => {}
        let s = "([...{x} = []]) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty(), "OXC should accept: {:?}", ret.diagnostics);
        let result = check_early_errors(&ret.program);
        assert!(result.is_err(), "nested rest init should be SyntaxError");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
    }

    #[test]
    fn arrow_dstr_rest_without_init_is_ok() {
        let s = "([...x]) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_early_errors(&ret.program);
        assert!(result.is_ok(), "rest without init should be ok: {:?}", result);
    }

    #[test]
    fn arrow_rest_without_default_is_ok() {
        let s = "(...x) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        assert!(ret.diagnostics.is_empty());
        let result = check_early_errors(&ret.program);
        assert!(result.is_ok(), "rest without default should be ok: {:?}", result);
    }

    // ===== Debug: check what OXC already catches =====

    #[test]
    fn oxc_check_duplicate_params_with_defaults() {
        let s = "(x = 1, x = 2) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        println!("OXC duplicate params+default: {} errors", ret.diagnostics.len());
        for e in &ret.diagnostics {
            println!("  OXC error: {:?}", e);
        }
        assert!(ret.diagnostics.is_empty(), "OXC should parse but our early errors check catches it");
    }

    #[test]
    fn oxc_check_rest_with_default() {
        let s = "(...x = []) => {}";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        println!("OXC rest+default: {} errors", ret.diagnostics.len());
        for e in &ret.diagnostics {
            println!("  OXC error: {:?}", e);
        }
        assert!(ret.diagnostics.is_empty(), "OXC should parse but our check catches it");
    }

    #[test]
    fn oxc_check_arrow_strict_body_with_array_destr() {
        let s = "([x]) => { 'use strict'; }";
        let source_type = SourceType::default().with_script(true).with_jsx(true);
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, s, source_type).parse();
        println!("OXC arrow strict body + array destr: {} errors", ret.diagnostics.len());
    }
}
