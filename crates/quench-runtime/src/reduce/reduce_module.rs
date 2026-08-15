//! Reduction of ES module declarations.
//!
//! Declarations are reduced through the ordinary statement path. Import
//! bindings reserve lexical slots; hosts populate those slots during linking.

use std::collections::HashMap;

use oxc::ast::ast::{
    ExportDefaultDeclaration, ExportDefaultDeclarationKind, ExportNamedDeclaration,
    ImportDeclaration, ImportDeclarationSpecifier, Statement,
};

use crate::statements;
use crate::{facts::ProgramDb, ops::Op};

macro_rules! reduce_module_exported_decl {
    (function $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {{
        crate::reduce::reduce_function_declaration(
            $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals,
        )
        .map(|_| None)
    }};
    (class $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {
        reduce_class(
            $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals,
        )
    };
    (variable $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {{
        statements::reduce_variable(
            &$declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals,
        )
    }};
}

macro_rules! reduce_exported_default {
    (expression $expression:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {{
        let result = crate::reduce::reduce_expression(
            $expression,
            $ops,
            $facts,
            $next_register,
            $locals,
        );
        if let Some(src) = result {
            let slot = *$next_slot;
            *$next_slot = $next_slot.saturating_add(1);
            $locals.insert("default".to_string(), slot);
            $ops.push(Op::DeclareEvalBinding {
                name: "default".to_string(),
                slot,
            });
            $ops.push(Op::StoreLocal { slot, src });
        }
        Ok(None)
    }};
    (function $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {
        reduce_module_exported_decl!(
            function $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals
        )
    };
    (class $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {
        reduce_module_exported_decl!(
            class $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals
        )
    };
}

pub(super) fn reduce_module_declaration(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        Statement::ImportDeclaration(import) => {
            reduce_import(import, ops, next_register, next_slot, locals)
        }
        Statement::ExportNamedDeclaration(export) => {
            reduce_named(export, ops, facts, next_register, next_slot, locals)
        }
        Statement::ExportDefaultDeclaration(export) => {
            reduce_default(export, ops, facts, next_register, next_slot, locals)
        }
        _ => Ok(None),
    }
}

fn reduce_import(
    import: &ImportDeclaration<'_>,
    ops: &mut Vec<Op>,
    _next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Some(specifiers) = &import.specifiers else {
        return Ok(None);
    };
    for specifier in specifiers {
        let local = match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(value) => &value.local,
            ImportDeclarationSpecifier::ImportDefaultSpecifier(value) => &value.local,
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(value) => &value.local,
        };
        let slot = locals.get(local.name.as_str()).copied().unwrap_or_else(|| {
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(local.name.to_string(), slot);
            slot
        });
        ops.push(Op::DeclareEvalBinding {
            name: local.name.to_string(),
            slot,
        });
    }
    Ok(None)
}

fn reduce_named(
    export: &ExportNamedDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if let Some(declaration) = &export.declaration {
        return reduce_named_declaration(declaration, ops, facts, next_register, next_slot, locals);
    }
    Ok(None)
}

fn reduce_named_declaration(
    declaration: &oxc::ast::ast::Declaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_named_declaration_kind(declaration, ops, facts, next_register, next_slot, locals)
}

fn reduce_named_declaration_kind(
    declaration: &oxc::ast::ast::Declaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match declaration {
        oxc::ast::ast::Declaration::FunctionDeclaration(function) => {
            reduce_named_function_declaration(
                function,
                ops,
                facts,
                next_register,
                next_slot,
                locals,
            )
        }
        oxc::ast::ast::Declaration::ClassDeclaration(class) => {
            reduce_named_class_declaration(class, ops, facts, next_register, next_slot, locals)
        }
        oxc::ast::ast::Declaration::VariableDeclaration(declaration) => {
            reduce_named_variable_declaration(
                declaration,
                ops,
                facts,
                next_register,
                next_slot,
                locals,
            )
        }
        _ => Ok(None),
    }
}

fn reduce_named_function_declaration(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_module_exported_decl!(
        function function,
        ops,
        facts,
        next_register,
        next_slot,
        locals
    )
}

fn reduce_named_class_declaration(
    class: &oxc::ast::ast::Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_module_exported_decl!(
        class class,
        ops,
        facts,
        next_register,
        next_slot,
        locals
    )
}

fn reduce_named_variable_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_module_exported_decl!(
        variable declaration,
        ops,
        facts,
        next_register,
        next_slot,
        locals
    )
}

fn reduce_default(
    export: &ExportDefaultDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if let Some(expression) = export.declaration.as_expression() {
        return reduce_default_expression(expression, ops, facts, next_register, next_slot, locals);
    }
    reduce_default_declaration(
        &export.declaration,
        ops,
        facts,
        next_register,
        next_slot,
        locals,
    )
}

fn reduce_default_expression(
    expression: &oxc::ast::ast::Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let result = crate::reduce::reduce_expression(expression, ops, facts, next_register, locals);
    if let Some(src) = result {
        if anonymous_default_definition(expression) && !class_has_own_name(expression) {
            ops.push(Op::SetFunctionName {
                function: src,
                name: "default".to_string(),
            });
        }
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert("default".to_string(), slot);
        ops.push(Op::DeclareEvalBinding {
            name: "default".to_string(),
            slot,
        });
        ops.push(Op::StoreLocal { slot, src });
    }
    Ok(None)
}

fn class_has_own_name(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    let mut expression = expression;
    while let oxc::ast::ast::Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    let oxc::ast::ast::Expression::ClassExpression(class) = expression else {
        return false;
    };
    class.body.body.iter().any(|element| {
        matches!(
            element,
            oxc::ast::ast::ClassElement::MethodDefinition(method)
                if method.r#static && matches!(&method.key, oxc::ast::ast::PropertyKey::StaticIdentifier(identifier) if identifier.name == "name")
        )
    })
}

fn anonymous_default_definition(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    let mut expression = expression;
    while let oxc::ast::ast::Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    matches!(
        expression,
        oxc::ast::ast::Expression::ArrowFunctionExpression(_)
    ) || matches!(
        expression,
        oxc::ast::ast::Expression::FunctionExpression(function) if function.id.is_none()
    ) || matches!(
        expression,
        oxc::ast::ast::Expression::ClassExpression(class) if class.id.is_none()
    )
}

fn reduce_default_declaration(
    declaration: &ExportDefaultDeclarationKind<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
            reduce_default_function_declaration(
                function,
                ops,
                facts,
                next_register,
                next_slot,
                locals,
            )
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            reduce_default_class_declaration(class, ops, facts, next_register, next_slot, locals)
        }
        _ => Ok(None),
    }
}

fn reduce_default_function_declaration<'a>(
    function: &'a oxc::ast::ast::Function<'a>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if function.id.is_none() {
        let src = crate::functions::reduce_expression(function, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported default declaration".to_string()])?;
        return store_default(src, ops, next_slot, locals);
    }
    reduce_exported_default!(
        function function,
        ops,
        facts,
        next_register,
        next_slot,
        locals
    )
}

fn reduce_default_class_declaration<'a>(
    class: &'a oxc::ast::ast::Class<'a>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if class.id.is_none() {
        let src = crate::classes::reduce_expression(class, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported default declaration".to_string()])?;
        return store_default(src, ops, next_slot, locals);
    }
    let result = reduce_module_exported_decl!(
        class class,
        ops,
        facts,
        next_register,
        next_slot,
        locals
    );
    if let Some(identifier) = class.id.as_ref() {
        if let Some(&src_slot) = locals.get(identifier.name.as_str()) {
            alias_default(src_slot, ops, next_register, next_slot, locals);
        }
    }
    result
}

fn alias_default(
    src_slot: u16,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) {
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert("default".to_string(), slot);
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::DeclareEvalBinding {
        name: "default".to_string(),
        slot,
    });
    ops.push(Op::LoadLocal {
        dst: register,
        slot: src_slot,
    });
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
}

fn store_default(
    src: u16,
    ops: &mut Vec<Op>,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    ops.push(Op::SetFunctionName {
        function: src,
        name: "default".to_string(),
    });
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert("default".to_string(), slot);
    ops.push(Op::DeclareEvalBinding {
        name: "default".to_string(),
        slot,
    });
    ops.push(Op::StoreLocal { slot, src });
    Ok(None)
}

fn reduce_class(
    class: &oxc::ast::ast::Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Some(identifier) = class.id.as_ref() else {
        return Err(vec!["Anonymous class declaration".to_string()]);
    };
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(identifier.name.to_string(), slot);
    let register = crate::classes::reduce_expression(class, ops, facts, next_register, locals)
        .ok_or_else(|| vec!["Unsupported class body".to_string()])?;
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(None)
}
