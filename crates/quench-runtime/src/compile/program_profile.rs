//! `QUENCH_MEMORY` attribution of a compiled program's owned bytes, one JSON line per
//! program: the function records and each per-function and program-wide table at capacity.
use crate::bytecode::{
    Atom, BindingSite, Constant, EnvironmentClone, FieldSite, Function, FunctionSlot, Handler,
    Instr,
    LocalRegister, MethodSite, ObjectSite, RegExpLiteralSite, Register, ResidualProgram,
    Superinstruction, WideInstruction,
};
use std::mem::size_of;

fn bytes<T>(values: &Vec<T>) -> usize {
    values.capacity() * size_of::<T>()
}

/// A thin slice's elements, which it holds at exactly their length.
fn slice_bytes<T>(values: &[T]) -> usize {
    size_of_val(values)
}

/// Each function's atom lists: names of parameters, locals, environment and globals.
fn atom_bytes(function: &Function) -> usize {
    [
        &*function.parameter_atoms,
        &*function.local_atoms,
        &*function.environment_atoms,
        &*function.lexical_atoms,
    ]
    .into_iter()
    .map(slice_bytes::<Atom>)
    .sum::<usize>()
        + function.global_declarations.as_deref().map_or(0, |declarations| {
            size_of::<crate::bytecode::GlobalDeclarations>()
                + [
                    &declarations.lexical,
                    &declarations.var,
                    &declarations.function,
                    &declarations.annex_b_var,
                    &declarations.immutable,
                ]
                .into_iter()
                .map(bytes)
                .sum::<usize>()
        })
}

/// Layout tables beside the code: registers, capture slots, binding sites and clones.
fn layout_bytes(function: &Function) -> usize {
    slice_bytes::<LocalRegister>(&function.local_registers)
        + function.selective_capture_slots.as_deref().map_or(0, slice_bytes::<u16>)
        + slice_bytes::<BindingSite>(&function.binding_sites)
        + slice_bytes::<EnvironmentClone>(&function.environment_clones)
}

pub(super) fn report(program: &ResidualProgram) {
    let slots = &program.functions;
    let records = || slots.iter().filter_map(FunctionSlot::record);
    let sum = |field: fn(&Function) -> usize| records().map(field).sum::<usize>();
    eprintln!(
        "{{\"kind\":\"quench-program-memory\",\"name\":{:?},\"source_bytes\":{},\"functions\":{},\
         \"function_record_size\":{},\"function_records\":{},\"code\":{},\"wide\":{},\
         \"positions\":{},\"handlers\":{},\"atom_lists\":{},\"layout\":{},\"constants\":{},\
         \"sites\":{},\"register_roots\":{}}}",
        program.source_name,
        program.source.as_ref().map_or(0, |source| source.text.len()),
        slots.len(),
        size_of::<Function>(),
        slots.capacity() * size_of::<FunctionSlot>() + records().count() * size_of::<Function>(),
        sum(|function| slice_bytes::<Instr>(&function.code)),
        sum(|function| bytes::<WideInstruction>(&function.wide)),
        sum(|function| function.source_positions.capacity_bytes()),
        sum(|function| slice_bytes::<Handler>(&function.handlers)),
        sum(atom_bytes),
        sum(layout_bytes),
        bytes::<Constant>(&program.constants),
        bytes::<MethodSite>(&program.method_sites)
            + bytes::<Register>(&program.method_arguments)
            + bytes::<FieldSite>(&program.field_sites)
            + bytes::<ObjectSite>(&program.object_sites)
            + bytes::<RegExpLiteralSite>(&program.regexp_literal_sites)
            + program
                .literal_templates
                .iter()
                .map(|steps| slice_bytes::<crate::bytecode::LiteralStep>(steps))
                .sum::<usize>()
            + bytes::<Superinstruction>(&program.superinstructions),
        sum(|function| function.register_roots.capacity_bytes()),
    );
}
