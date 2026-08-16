//! Small reduction helpers shared across the reducer.

use std::collections::HashMap;

pub(crate) fn function_strictness(
    body: &oxc::ast::ast::FunctionBody<'_>,
    inherited: bool,
) -> crate::ops::FunctionStrictness {
    if inherited
        || body
            .directives
            .iter()
            .any(|directive| directive.directive.as_str() == "use strict")
    {
        crate::ops::FunctionStrictness::Strict
    } else {
        crate::ops::FunctionStrictness::Sloppy
    }
}

use crate::ops::{Constant, Op};

const SCRIPT_THIS_SLOT: &str = "\0script_this";
const DYNAMIC_IMPORT_SYNTAX_ERROR: &str = "SyntaxError: Invalid dynamic import usage";

pub(crate) fn has_strict_directive(program: &oxc::ast::ast::Program<'_>) -> bool {
    program
        .directives
        .iter()
        .any(|directive| directive.directive.as_str() == "use strict")
}

pub(crate) fn validate_parse(parsed: &oxc::parser::ParserReturn<'_>) -> Result<(), Vec<String>> {
    if parsed.panicked {
        return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
    }
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|error| format!("SyntaxError: {error}"))
            .collect());
    }
    validate_program(&parsed.program)
}

struct RegexpLiteralValidator<'a> {
    errors: Vec<String>,
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> oxc::ast::visit::Visit<'a> for RegexpLiteralValidator<'a> {
    fn visit_reg_exp_literal(&mut self, literal: &oxc::ast::ast::RegExpLiteral<'a>) {
        if let Some(raw) = literal.raw.as_ref() {
            let text = raw.as_str();
            if let Some(separator) = text.rfind('/') {
                let pattern = &text[1..separator];
                if let Err(error) = crate::regexp::validate_literal(pattern) {
                    self.errors.push(error);
                }
                if let Err(error) = crate::regexp::validate_pattern(pattern) {
                    self.errors.push(error);
                }
                let flags = &text[separator + 1..];
                if flags.contains('u') {
                    if let Err(error) = crate::regexp::validate_unicode(pattern, flags) {
                        self.errors.push(error);
                    }
                }
            }
        }
    }
}

struct DynamicImportValidator<'a> {
    errors: Vec<String>,
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> DynamicImportValidator<'a> {
    fn validate_new_import_usage(&mut self, callee: &oxc::ast::ast::Expression<'a>) {
        if matches!(callee, oxc::ast::ast::Expression::ImportExpression(_)) {
            self.errors.push(DYNAMIC_IMPORT_SYNTAX_ERROR.to_string());
        }
        if let oxc::ast::ast::Expression::StaticMemberExpression(member) = callee {
            if matches!(
                &member.object,
                oxc::ast::ast::Expression::ImportExpression(_)
            ) {
                self.errors.push(DYNAMIC_IMPORT_SYNTAX_ERROR.to_string());
            }
            return;
        }
        if let oxc::ast::ast::Expression::ComputedMemberExpression(member) = callee {
            if matches!(
                &member.object,
                oxc::ast::ast::Expression::ImportExpression(_)
            ) {
                self.errors.push(DYNAMIC_IMPORT_SYNTAX_ERROR.to_string());
            }
        }
    }
}

impl<'a> oxc::ast::visit::Visit<'a> for DynamicImportValidator<'a> {
    fn visit_new_expression(&mut self, expression: &oxc::ast::ast::NewExpression<'a>) {
        self.validate_new_import_usage(&expression.callee);
        self.visit_expression(&expression.callee);
        for argument in &expression.arguments {
            self.visit_argument(argument);
        }
    }
}

pub(crate) fn validate_program(program: &oxc::ast::ast::Program<'_>) -> Result<(), Vec<String>> {
    let mut regexp_validator = RegexpLiteralValidator {
        errors: Vec::new(),
        marker: std::marker::PhantomData,
    };
    oxc::ast::visit::walk::walk_program(&mut regexp_validator, program);
    if !regexp_validator.errors.is_empty() {
        return Err(regexp_validator.errors);
    }
    let mut import_validator = DynamicImportValidator {
        errors: Vec::new(),
        marker: std::marker::PhantomData,
    };
    oxc::ast::visit::walk::walk_program(&mut import_validator, program);
    if import_validator.errors.is_empty() {
        Ok(())
    } else {
        Err(import_validator.errors)
    }
}

/// Highest allocated local slot plus one, used as the next register base.
pub(crate) fn register_base(locals: &HashMap<String, u16>) -> u16 {
    locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1))
}

/// Reserve function-scoped bindings before reducing executable statements.
pub(crate) fn predeclare_functions(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        let names = if matches!(statement, oxc::ast::ast::Statement::FunctionDeclaration(_)) {
            declared_names(statement)
        } else {
            nested_var_names(statement)
        };
        for name in names {
            reserve(&name, locals, next_slot);
        }
    }
}

fn nested_var_names(statement: &oxc::ast::ast::Statement<'_>) -> Vec<String> {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(_) => Vec::new(),
        oxc::ast::ast::Statement::BlockStatement(block) => annex_b_function_names(&block.body),
        oxc::ast::ast::Statement::IfStatement(statement) => {
            let mut names = annex_b_function_names(std::slice::from_ref(&statement.consequent));
            if let Some(alternate) = &statement.alternate {
                names.extend(annex_b_function_names(std::slice::from_ref(alternate)));
            }
            names
        }
        oxc::ast::ast::Statement::SwitchStatement(statement) => statement
            .cases
            .iter()
            .flat_map(|case| annex_b_function_names(&case.consequent))
            .collect(),
        _ => declared_names(statement),
    }
}

pub(crate) fn instantiate_script_declarations(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
    ops: &mut Vec<Op>,
    script: bool,
) -> Vec<(String, bool)> {
    predeclare_functions(statements, locals, next_slot);
    if !script {
        return Vec::new();
    }
    let mut lexical = Vec::new();
    for statement in statements {
        for (name, immutable) in script_lexical_names(statement) {
            ops.push(Op::CheckGlobalVar {
                name: name.clone(),
                is_lexical: true,
                is_eval: false,
            });
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(name.clone(), slot);
            ops.push(Op::MarkUninitialized { slot });
            lexical.push((name, immutable));
        }
    }
    lexical
}

pub(crate) fn script_lexical_slot(
    statement: &oxc::ast::ast::Statement<'_>,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    script_lexical_names(statement)
        .first()
        .and_then(|(name, _)| locals.get(name).copied())
}

fn script_lexical_names(statement: &oxc::ast::ast::Statement<'_>) -> Vec<(String, bool)> {
    match statement {
        oxc::ast::ast::Statement::VariableDeclaration(declaration)
            if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var =>
        {
            declaration
                .declarations
                .iter()
                .flat_map(|declarator| {
                    let immutable =
                        declaration.kind == oxc::ast::ast::VariableDeclarationKind::Const;
                    crate::binding_patterns::names(&declarator.id)
                        .into_iter()
                        .map(move |name| (name, immutable))
                })
                .collect()
        }
        oxc::ast::ast::Statement::ClassDeclaration(class) => class
            .id
            .iter()
            .map(|identifier| (identifier.name.to_string(), false))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn reserve_names(
    names: &[String],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) -> Vec<(String, u16)> {
    let mut names = names.to_vec();
    names.sort_unstable();
    names.dedup();
    let mut declared = Vec::new();
    for name in names {
        if locals.contains_key(&name) {
            continue;
        }
        let slot = *next_slot;
        reserve(&name, locals, next_slot);
        declared.push((name, slot));
    }
    declared
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvalBehavior {
    Normal,
    Strict,
    Local,
    Global,
    Script,
}

pub(crate) type EvalBindings = (
    HashMap<String, u16>,
    u16,
    Vec<Op>,
    EvalBehavior,
    Vec<(String, u16)>,
);

include!("reduce_support_bindings.rs");
