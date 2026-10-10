use std::rc::Rc;

use crate::bytecode::{Function, ImmediateLayout, Op};

pub(super) fn apply(functions: &mut [Function], atoms: &[Rc<str>]) {
    let captured = captured_slots(functions);
    let dynamic_ancestors = dynamic_scope_ancestors(functions);

    for (id, function) in functions.iter_mut().enumerate() {
        if !has_closure(function) {
            continue;
        }

        let can_split = selective_layout_is_safe(function, id, dynamic_ancestors[id], atoms);
        if can_split {
            function.selective_capture_slots = Some(captured[id].clone());
            rewrite_local_ownership(function, &captured[id]);
        } else {
            function.selective_capture_slots = None;
            rewrite_all_locals_to_environment(function);
        }
    }
}

fn captured_slots(functions: &[Function]) -> Vec<Vec<u16>> {
    let mut captured = functions.iter().map(|_| Vec::new()).collect::<Vec<_>>();

    for (id, function) in functions.iter().enumerate() {
        for instruction in &function.code {
            if !matches!(instruction.op(), Op::LoadCapture | Op::StoreCapture) {
                continue;
            }
            let depth = usize::from(instruction.capture_depth()) + 1;
            let slot = instruction.capture_slot();
            if let Some(owner) = ancestor(functions, id, depth)
                && usize::from(slot) < usize::from(functions[owner].locals)
            {
                captured[owner].push(slot);
            }
        }
        for instruction in &function.wide {
            if !matches!(instruction.op(), Op::LoadCapture | Op::StoreCapture) {
                continue;
            }
            let depth = usize::from(instruction.capture_depth()) + 1;
            let slot = instruction.capture_slot();
            if let Some(owner) = ancestor(functions, id, depth)
                && usize::from(slot) < usize::from(functions[owner].locals)
            {
                captured[owner].push(slot);
            }
        }
    }

    for (id, function) in functions.iter().enumerate() {
        if function.arguments_are_mapped()
            && let Some(arguments_slot) = function.arguments_slot
            && captured[id].iter().any(|slot| *slot < function.params)
        {
            captured[id].push(arguments_slot);
        }
    }

    for slots in &mut captured {
        slots.sort_unstable();
        slots.dedup();
    }
    captured
}

fn dynamic_scope_ancestors(functions: &[Function]) -> Vec<bool> {
    let inherited_dynamic_scope = inherited_dynamic_scope(functions);
    let mut unsafe_layout = vec![false; functions.len()];
    for (id, function) in functions.iter().enumerate() {
        if !has_dynamic_scope_access(function) && !inherited_dynamic_scope[id] {
            continue;
        }
        let mut current = Some(id);
        while let Some(owner) = current {
            unsafe_layout[owner] = true;
            current = functions[owner].parent.map(|parent| parent as usize);
        }
    }
    unsafe_layout
}

fn inherited_dynamic_scope(functions: &[Function]) -> Vec<bool> {
    let mut inherited = vec![false; functions.len()];
    for (id, function) in functions.iter().enumerate() {
        let Some(parent) = function.parent.map(|parent| parent as usize) else {
            continue;
        };
        inherited[id] = inherited[parent] || !functions[parent].binding_sites.is_empty();
    }
    inherited
}

fn has_dynamic_scope_access(function: &Function) -> bool {
    function.inherited_with_scope
        || !function.binding_sites.is_empty()
        || function
            .code
            .iter()
            .any(|instruction| dynamic_scope_opcode(instruction.op(), instruction.imm()))
        || function
            .wide
            .iter()
            .any(|instruction| dynamic_scope_opcode(instruction.op(), instruction.imm()))
}

fn dynamic_scope_opcode(op: Op, immediate: u32) -> bool {
    matches!(op, Op::ResolveName | Op::CallDirectEvalArray)
        || (op == Op::Call
            && (ImmediateLayout::direct_eval(immediate)
                || ImmediateLayout::parameter_eval(immediate)))
}

fn selective_layout_is_safe(
    function: &Function,
    id: usize,
    has_dynamic_descendant: bool,
    atoms: &[Rc<str>],
) -> bool {
    id != 0
        && function.parent.is_some()
        && !has_dynamic_descendant
        && !function.is_async
        && !function.is_generator
        && !function.is_class_constructor
        && !function.derived_constructor
        && !function.class_field_initializer
        && function.simple_parameters
        && function.self_binding_slot.is_none()
        && function.environment_clones.is_empty()
        && function.lexical_atoms.is_empty()
        && function.local_atoms.iter().all(|atom| {
            !atoms
                .get(*atom as usize)
                .is_some_and(|name| name.starts_with('\0'))
        })
        && function
            .code
            .iter()
            .all(|instruction| !matches!(instruction.op(), Op::InitializeTdz | Op::CloneEnv))
        && function
            .wide
            .iter()
            .all(|instruction| !matches!(instruction.op(), Op::InitializeTdz | Op::CloneEnv))
        && !has_dynamic_scope_access(function)
}

fn has_closure(function: &Function) -> bool {
    function
        .code
        .iter()
        .any(|instruction| instruction.op() == Op::MakeClosure)
        || function
            .wide
            .iter()
            .any(|instruction| instruction.op() == Op::MakeClosure)
}

fn rewrite_local_ownership(function: &mut Function, captured: &[u16]) {
    for instruction in &mut function.code {
        rewrite_if_captured(instruction, captured);
    }
    for instruction in &mut function.wide {
        if matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal)
            && u16::try_from(instruction.local_slot())
                .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
        {
            instruction.set_op(if instruction.op() == Op::LoadLocal {
                Op::LoadEnvLocal
            } else {
                Op::StoreEnvLocal
            });
        }
    }
}

fn rewrite_if_captured(instruction: &mut crate::bytecode::Instr, captured: &[u16]) {
    if !matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal)
        || !u16::try_from(instruction.local_slot())
            .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
    {
        return;
    }
    instruction.set_op(if instruction.op() == Op::LoadLocal {
        Op::LoadEnvLocal
    } else {
        Op::StoreEnvLocal
    });
}

fn rewrite_all_locals_to_environment(function: &mut Function) {
    for instruction in &mut function.code {
        if matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal) {
            instruction.set_op(if instruction.op() == Op::LoadLocal {
                Op::LoadEnvLocal
            } else {
                Op::StoreEnvLocal
            });
        }
    }
    for instruction in &mut function.wide {
        if matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal) {
            instruction.set_op(if instruction.op() == Op::LoadLocal {
                Op::LoadEnvLocal
            } else {
                Op::StoreEnvLocal
            });
        }
    }
}

fn ancestor(functions: &[Function], mut function: usize, distance: usize) -> Option<usize> {
    for _ in 0..distance {
        function = functions.get(function)?.parent? as usize;
    }
    Some(function)
}
