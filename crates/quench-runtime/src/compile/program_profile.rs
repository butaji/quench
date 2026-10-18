//! `QUENCH_MEMORY` attribution of a compiled program's owned bytes, one JSON line per
//! program: the function records and each per-function and program-wide table at capacity.
use crate::bytecode::{
    Atom, BindingSite, Constant, EnvironmentClone, FieldSite, Function, Handler, Instr,
    LocalRegister, MethodSite, ObjectSite, RegExpLiteralSite, Register, ResidualProgram,
    Superinstruction, WideInstruction,
};
use std::mem::size_of;

fn bytes<T>(values: &Vec<T>) -> usize {
    values.capacity() * size_of::<T>()
}

/// Each function's atom lists: names of parameters, locals, environment and globals.
fn atom_bytes(function: &Function) -> usize {
    [
        &function.parameter_atoms,
        &function.local_atoms,
        &function.environment_atoms,
        &function.lexical_atoms,
        &function.global_lexical_atoms,
        &function.global_var_atoms,
        &function.global_function_atoms,
        &function.global_annex_b_var_atoms,
        &function.global_immutable_atoms,
    ]
    .into_iter()
    .map(|atoms| size_of_val::<[Atom]>(atoms))
    .sum()
}

/// Layout tables beside the code: registers, capture slots, binding sites and clones.
fn layout_bytes(function: &Function) -> usize {
    bytes::<LocalRegister>(&function.local_registers)
        + function.selective_capture_slots.as_ref().map_or(0, bytes::<u16>)
        + bytes::<BindingSite>(&function.binding_sites)
        + bytes::<EnvironmentClone>(&function.environment_clones)
}

pub(super) fn report(program: &ResidualProgram) {
    let functions = &program.functions;
    let sum = |field: fn(&Function) -> usize| functions.iter().map(field).sum::<usize>();
    eprintln!(
        "{{\"kind\":\"quench-program-memory\",\"name\":{:?},\"source_bytes\":{},\"functions\":{},\
         \"function_record_size\":{},\"function_records\":{},\"code\":{},\"wide\":{},\
         \"positions\":{},\"handlers\":{},\"atom_lists\":{},\"layout\":{},\"constants\":{},\
         \"sites\":{},\"register_roots\":{}}}",
        program.source_name,
        program.source.as_ref().map_or(0, |source| source.text.len()),
        functions.len(),
        size_of::<Function>(),
        functions.capacity() * size_of::<Function>(),
        sum(|function| bytes::<Instr>(&function.code)),
        sum(|function| bytes::<WideInstruction>(&function.wide)),
        sum(|function| function.source_positions.capacity_bytes()),
        sum(|function| bytes::<Handler>(&function.handlers)),
        sum(atom_bytes),
        sum(layout_bytes),
        bytes::<Constant>(&program.constants),
        bytes::<MethodSite>(&program.method_sites)
            + bytes::<Register>(&program.method_arguments)
            + bytes::<FieldSite>(&program.field_sites)
            + bytes::<ObjectSite>(&program.object_sites)
            + bytes::<RegExpLiteralSite>(&program.regexp_literal_sites)
            + bytes::<Superinstruction>(&program.superinstructions),
        program.register_roots.capacity_bytes(),
    );
}
