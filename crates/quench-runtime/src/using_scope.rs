use std::collections::HashMap;

use oxc::ast::ast::{Statement, VariableDeclarationKind};

use crate::ops::{Builtin, Op};

pub(crate) const CAPABILITY: &str = "\0dispose-capability";

pub(crate) fn has_using(statements: &[Statement<'_>]) -> bool {
    statements.iter().any(statement_has_using)
}

pub(crate) fn has_await_using(statements: &[Statement<'_>]) -> bool {
    statements.iter().any(statement_has_await_using)
}

pub(crate) fn reserve_slot(locals: &mut HashMap<String, u16>, next_slot: &mut u16) -> u16 {
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(CAPABILITY.to_string(), slot);
    slot
}

pub(crate) fn reserve(
    statements: &[Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) -> Option<u16> {
    if !has_using(statements) {
        return None;
    }
    Some(reserve_slot(locals, next_slot))
}

pub(crate) fn emit_tdz(
    statements: &[Statement<'_>],
    ops: &mut Vec<Op>,
    locals: &HashMap<String, u16>,
) {
    for statement in statements {
        for name in crate::reduce_support::lexical_bound_names(statement) {
            let Some(&slot) = locals.get(&name) else {
                continue;
            };
            ops.push(Op::MarkUninitialized { slot, shared: true });
            if crate::reduce_support::lexical_declaration(statement).is_some_and(|declaration| {
                matches!(
                    declaration.kind,
                    oxc::ast::ast::VariableDeclarationKind::Const
                        | oxc::ast::ast::VariableDeclarationKind::Using
                        | oxc::ast::ast::VariableDeclarationKind::AwaitUsing
                )
            }) {
                ops.push(Op::MarkImmutable { slot });
            }
        }
    }
}

pub(crate) fn emit_create(
    ops: &mut Vec<Op>,
    stack: u16,
    await_using: bool,
    next_register: &mut u16,
) {
    let ctor = take(next_register);
    let value = take(next_register);
    ops.push(Op::MakeBuiltin {
        dst: ctor,
        builtin: if await_using {
            Builtin::AsyncDisposableStack
        } else {
            Builtin::DisposableStack
        },
    });
    ops.push(Op::Construct {
        dst: value,
        callee: ctor,
        args: Vec::new(),
        spreads: Vec::new(),
    });
    ops.push(Op::StoreLocal {
        slot: stack,
        src: value,
    });
}

pub(crate) fn wrap(
    body: Vec<Op>,
    stack: u16,
    await_using: bool,
    _next_register: &mut u16,
) -> Result<Vec<Op>, Vec<String>> {
    Ok(vec![Op::WithDispose {
        body: crate::machine::FunctionCode::from_ops(body),
        stack,
        await_using,
    }])
}

pub(crate) fn execute(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Op::WithDispose {
        body,
        stack,
        await_using,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(ops) = body.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let completion = crate::exceptions::execute_ops(ops, registers)?;
    crate::disposable_stack::dispose_completion(registers, *stack, completion, *await_using)
}

pub(crate) fn register_resource(
    kind: VariableDeclarationKind,
    resource: u16,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) {
    if !matches!(
        kind,
        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
    ) {
        return;
    }
    let Some(&stack) = locals.get(CAPABILITY) else {
        return;
    };
    let dst = take(next_register);
    let object = take(next_register);
    ops.push(Op::LoadLocal {
        dst: object,
        slot: stack,
    });
    ops.push(Op::CallMethod {
        dst,
        object,
        key: "use".to_string(),
        callee: None,
        args: vec![resource],
    });
}

pub(crate) fn mark_binding_tdz(
    pattern: &oxc::ast::ast::BindingPattern<'_>,
    ops: &mut Vec<Op>,
    locals: &HashMap<String, u16>,
) {
    for name in crate::binding_patterns::names(pattern) {
        if let Some(&slot) = locals.get(&name) {
            ops.push(Op::MarkUninitialized { slot, shared: true });
        }
    }
}

pub(crate) fn mark_binding_immutable(
    kind: VariableDeclarationKind,
    pattern: &oxc::ast::ast::BindingPattern<'_>,
    ops: &mut Vec<Op>,
    locals: &HashMap<String, u16>,
) {
    if !matches!(
        kind,
        VariableDeclarationKind::Const
            | VariableDeclarationKind::Using
            | VariableDeclarationKind::AwaitUsing
    ) {
        return;
    }
    for name in crate::binding_patterns::names(pattern) {
        if let Some(&slot) = locals.get(&name) {
            ops.push(Op::MarkImmutable { slot });
        }
    }
}

pub(crate) fn for_init_kind(
    init: Option<&oxc::ast::ast::ForStatementInit<'_>>,
) -> Option<VariableDeclarationKind> {
    let oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration) = init? else {
        return None;
    };
    is_using_kind(declaration.kind).then_some(declaration.kind)
}

pub(crate) fn is_using_kind(kind: VariableDeclarationKind) -> bool {
    matches!(
        kind,
        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
    )
}

fn statement_has_using(statement: &Statement<'_>) -> bool {
    let Statement::VariableDeclaration(declaration) = statement else {
        return false;
    };
    is_using_kind(declaration.kind)
}

fn statement_has_await_using(statement: &Statement<'_>) -> bool {
    let Statement::VariableDeclaration(declaration) = statement else {
        return false;
    };
    declaration.kind == VariableDeclarationKind::AwaitUsing
}

fn take(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}
