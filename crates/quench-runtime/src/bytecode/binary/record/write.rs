//! The writer of function records: the groups `record.rs` reads, written in the same order.

use super::*;

/// Writes `function`'s record, with `derived_facts` as its last field (not in packed records).
pub(super) fn write_function(out: &mut BinaryWriter, function: &Function, superinstructions: &[Superinstruction], derived_facts: u32) {
    let start = out.bytes.len();
    write_head(out, function, start);
    write_flags(out, function);
    write_scope(out, function);
    write_registers(out, function);
    write_code(out, function);
    write_tail(out, function, superinstructions, derived_facts);
}

fn write_head(out: &mut BinaryWriter, function: &Function, start: usize) {
    out.option_u32(function.parent);
    out.option_atom(function.name);
    debug_assert!(out.packed || out.bytes.len() - start == RECORD_INFERRED_NAME_AT);
    out.option_atom(function.inferred_name);
    out.u8(u8::from(function.is_arrow));
    out.u16(function.self_binding_slot.unwrap_or(u16::MAX));
    out.u8(u8::from(function.source_span.is_some()));
    if let Some(span) = function.source_span {
        out.u32(span.start);
        out.u32(span.end);
    }
    out.u16(function.params);
    out.u16(function.length);
    out.u32(function.parameter_end_pc);
    write_atoms(out, &function.parameter_atoms);
}

fn write_atoms(out: &mut BinaryWriter, atoms: &[Atom]) {
    out.u32(atoms.len() as u32);
    for atom in atoms {
        out.atom(*atom);
    }
}

fn write_flags(out: &mut BinaryWriter, function: &Function) {
    out.u8(u8::from(function.rest));
    out.u8(u8::from(function.is_async));
    out.u8(u8::from(function.is_generator));
    out.u8(u8::from(function.is_class_constructor));
    out.u8(u8::from(function.derived_constructor));
    out.option_u32(function.instance_initializer);
    out.option_atom(function.super_home_atom);
    out.u8(u8::from(function.constructible));
    out.u8(u8::from(function.class_initializer));
    out.u8(u8::from(function.prototype_unobservable));
    out.u8(u8::from(function.parameter_eval_arguments_error));
    out.u8(u8::from(function.unaliased_arguments));
    out.u8(u8::from(function.simple_parameters));
    out.u8(u8::from(function.strict));
    out.u16(function.arguments_slot.unwrap_or(u16::MAX));
}

fn write_scope(out: &mut BinaryWriter, function: &Function) {
    out.u16(function.locals);
    write_atoms(out, &function.local_atoms);
    out.u32(function.local_registers.len() as u32);
    for local_register in &function.local_registers {
        out.u16(local_register.local);
        out.u16(local_register.register);
    }
    write_atoms(out, &function.environment_atoms);
    // A packed record's bindings stay beside it, shared with its scope (`PackedUnlowered`).
    let name_bindings = if out.packed { Vec::new() } else { function.name_bindings.resolved() };
    out.u32(name_bindings.len() as u32);
    for binding in &name_bindings {
        write_eval_binding(out, binding);
    }
    function.environment_clones.write(out);
    match &function.selective_capture_slots {
        Some(slots) => {
            out.u8(1);
            out.u16s(slots);
        }
        None => out.u8(0),
    }
    out.u8(u8::from(function.inherited_with_scope));
    write_atoms(out, &function.lexical_atoms);
    write_atoms(out, function.global_lexical_atoms());
    write_atoms(out, function.global_var_atoms());
    write_atoms(out, function.global_function_atoms());
    write_atoms(out, function.global_annex_b_var_atoms());
    write_atoms(out, function.global_immutable_atoms());
    out.u32(function.binding_sites.len() as u32);
    for site in &function.binding_sites {
        out.u32(site.resume_pc);
        out.u32(site.bindings.len() as u32);
        for binding in &site.bindings {
            write_eval_binding(out, binding);
        }
    }
}

fn write_registers(out: &mut BinaryWriter, function: &Function) {
    out.u16(function.registers);
    out.u8(function.dispatch as u8);
    out.u8(u8::from(function.register_roots.covered()));
    out.u16(function.register_roots.entry_live());
    let dead_call_results = function.register_roots.dead_call_results();
    out.u32(dead_call_results.len() as u32);
    for &pc in dead_call_results {
        out.u32(pc);
    }
    out.u16(function.undefined_register.unwrap_or(NO_UNDEFINED_REGISTER));
    out.u16(function.parameter_registers.unwrap_or(NO_PARAMETER_REGISTERS));
    out.u8(function.initial_register as u8);
}

fn write_code(out: &mut BinaryWriter, function: &Function) {
    out.u32(function.code.len() as u32);
    for instruction in &function.code {
        out.instruction(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm());
    }
    out.u32(function.wide.len() as u32);
    for instruction in &function.wide {
        out.instruction(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm());
    }
}

fn write_tail(out: &mut BinaryWriter, function: &Function, superinstructions: &[Superinstruction], derived_facts: u32) {
    out.u32(function.handlers.len() as u32);
    for handler in &function.handlers {
        write_handler(out, handler);
    }
    out.source_positions(&function.source_positions);
    if out.packed {
        return;
    }
    let plain = match function.plain_locals.get() {
        Some(words) => words.to_vec(),
        None => function.plain_local_slots(superinstructions).words().to_vec(),
    };
    for byte in 0..usize::from(function.locals).div_ceil(u8::BITS as usize) {
        out.u8((plain[byte / size_of::<u64>()] >> (byte % size_of::<u64>() * u8::BITS as usize)) as u8);
    }
    out.u32(derived_facts);
}

fn write_handler(out: &mut BinaryWriter, handler: &Handler) {
    out.u32(handler.start);
    out.u32(handler.end);
    out.u32(handler.target);
    let (kind, slot) = match handler.slot {
        None => (THROWN_VALUE_DISCARDED, u16::MAX),
        Some(super::super::ThrownValueSlot::Local(slot)) => (THROWN_VALUE_IN_LOCAL, slot),
        Some(super::super::ThrownValueSlot::Register(register)) => (THROWN_VALUE_IN_REGISTER, register),
    };
    out.u8(kind);
    out.u16(slot);
    out.u32(handler.return_target.unwrap_or(u32::MAX));
    out.u16(handler.return_slot.unwrap_or(u16::MAX));
    out.u16(handler.with_depth);
}
