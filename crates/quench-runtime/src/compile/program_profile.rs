//! A compiled program's owned bytes (`ProgramFootprint`): what the collector weighs a
//! reclaimable program by, and what `QUENCH_MEMORY` attributes, one JSON line per program.
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

/// A compiled program's owned bytes by part: its function records and each per-function and
/// program-wide table at capacity. The collector weighs a reclaimable program by its total
/// (`Heap::charge_reclaimable_program`); `QUENCH_MEMORY` reports the parts.
pub(crate) struct ProgramFootprint {
    source: usize,
    function_records: usize,
    code: usize,
    wide: usize,
    positions: usize,
    handlers: usize,
    atom_lists: usize,
    layout: usize,
    constants: usize,
    sites: usize,
    register_roots: usize,
}

impl ProgramFootprint {
    pub(crate) fn of(program: &ResidualProgram) -> Self {
        let slots = &program.functions;
        let records = || slots.iter().filter_map(FunctionSlot::record);
        let sum = |field: fn(&Function) -> usize| records().map(field).sum::<usize>();
        Self {
            source: program.source.as_ref().map_or(0, |source| source.text.len()),
            function_records: slots.capacity() * size_of::<FunctionSlot>()
                + records().count() * size_of::<Function>(),
            code: sum(|function| bytes::<Instr>(&function.code)),
            wide: sum(|function| bytes::<WideInstruction>(&function.wide)),
            positions: sum(|function| function.source_positions.capacity_bytes()),
            handlers: sum(|function| slice_bytes::<Handler>(&function.handlers)),
            atom_lists: sum(atom_bytes),
            layout: sum(layout_bytes),
            constants: bytes::<Constant>(&program.constants),
            sites: bytes::<MethodSite>(&program.method_sites)
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
            register_roots: sum(|function| function.register_roots.capacity_bytes()),
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.source
            + self.function_records
            + self.code
            + self.wide
            + self.positions
            + self.handlers
            + self.atom_lists
            + self.layout
            + self.constants
            + self.sites
            + self.register_roots
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn report(program: &ResidualProgram) {
        let footprint = Self::of(program);
        eprintln!(
            "{{\"kind\":\"quench-program-memory\",\"name\":{:?},\"source_bytes\":{},\"functions\":{},\
             \"function_record_size\":{},\"function_records\":{},\"code\":{},\"wide\":{},\
             \"positions\":{},\"handlers\":{},\"atom_lists\":{},\"layout\":{},\"constants\":{},\
             \"sites\":{},\"register_roots\":{}}}",
            program.source_name,
            footprint.source,
            program.functions.len(),
            size_of::<Function>(),
            footprint.function_records,
            footprint.code,
            footprint.wide,
            footprint.positions,
            footprint.handlers,
            footprint.atom_lists,
            footprint.layout,
            footprint.constants,
            footprint.sites,
            footprint.register_roots,
        );
    }
}
