//! Shared helpers of the AOT facts tests: programs, lookups and the facts each test reads.

use quench_runtime::staged::glue_prelude::*;
use quench_runtime::compile::aot_facts::{analyze, AotFacts, BuiltinPath};
use quench_runtime::Engine;
use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::staged::Operation;
use quench_runtime::typed_ir::{Cell, ElementClass, FactKey, Identity, Type};

pub(crate) fn program(source: &str) -> ResidualProgram {
    Engine::specialize(source, "aot-facts.js").unwrap()
}

/// The id of the function named `name`.
pub(crate) fn function_named(program: &ResidualProgram, name: &str) -> usize {
    let named = |id: usize| {
        program.with_record(id, |function| {
            function.name.is_some_and(|atom| {
                let text: &str = program.atoms[atom as usize].as_ref();
                text == name
            })
        })
    };
    (0..program.function_count()).find(|&id| named(id)).expect("a function with that name")
}

/// The settled cell of parameter `index` of the function named `name`.
pub(crate) fn parameter(program: &ResidualProgram, facts: &quench_runtime::compile::aot_facts::AotFacts, name: &str, index: u16) -> Cell {
    let function = function_named(program, name) as u32;
    facts.cell(FactKey::Parameter { function, index }).expect("a settled parameter")
}

/// The cells the registers of function `name` hold before its last instruction (its return), where the solve settled them.
pub(crate) fn cells_before_return(facts: &AotFacts, program: &ResidualProgram, name: &str) -> Vec<Cell> {
    let id = function_named(program, name);
    let (last, registers) = program.with_record(id, |function| (function.code.len() as u32 - 1, usize::from(function.registers)));
    let function = facts.function_facts(id as u32);
    (0..registers).filter_map(|register| function.cell_at(last, register as u16)).collect()
}

/// The element classes other than the unnamed one that share its elements (`AotFacts::element_class_keys`).
pub(crate) fn classes_sharing_unnamed(facts: &AotFacts) -> Vec<FactKey> {
    let unnamed = FactKey::Elements(ElementClass::Unnamed);
    facts.cells.keys().copied().filter(|key| *key != unnamed && facts.element_class_keys(*key).contains(&unnamed)).collect()
}

/// The atom of the property name `name`, which the program names.
pub(crate) fn atom_named(program: &ResidualProgram, name: &str) -> u32 {
    atom_of(program, name)
}

/// A program whose constructor makes an array after its first store, and that stores a computed name on an array
/// elsewhere. `extra` adds the code that makes the global object a value.
pub(crate) fn array_store_program(extra: &str) -> String {
    format!(
        "function P(n) {{ this.a = 1; this.b = new Array(2); }} function fill(obj, k, x) {{ obj.list[k] = x; }} \
         function make() {{ var o = {{ list: [] }}; fill(o, 'q', 1); return new P(0); }} {extra} make();"
    )
}

/// The pc, index register and array register of each element access (a read or a store) of function `id`.
pub(crate) fn element_accesses(program: &quench_runtime::bytecode::ResidualProgram, id: usize) -> Vec<(u32, u16, u16)> {
    program.with_record(id, |function| {
        (0..function.code.len()).filter_map(|pc| match Operation::<u64, ()>::of(function.instruction_at(pc))? {
            Operation::ElementRead { base, key, .. } => Some((pc as u32, key.register_index()?, base)),
            Operation::ElementWrite { base, key, .. } => Some((pc as u32, key, base)),
            _ => None,
        }).collect()
    })
}

pub(crate) fn union_names(union: &quench_runtime::compile::aot_facts::UnionLayout) -> Vec<u32> {
    union.fields.iter().map(|(atom, _)| *atom).collect()
}

/// The pcs of the element stores (`ElementWrite`) of function `id`.
pub(crate) fn element_stores(program: &quench_runtime::bytecode::ResidualProgram, id: usize) -> Vec<u32> {
    program.with_record(id, |function| {
        (0..function.code.len()).filter(|&pc| matches!(Operation::<u64, ()>::of(function.instruction_at(pc)), Some(Operation::ElementWrite { .. })))
            .map(|pc| pc as u32).collect()
    })
}

/// The pcs of the element reads (`ElementRead`) of function `id`.
pub(crate) fn element_reads(program: &quench_runtime::bytecode::ResidualProgram, id: usize) -> Vec<u32> {
    program.with_record(id, |function| {
        (0..function.code.len()).filter(|&pc| matches!(Operation::<u64, ()>::of(function.instruction_at(pc)), Some(Operation::ElementRead { .. })))
            .map(|pc| pc as u32).collect()
    })
}

/// The atom of the name `text` in `program`.
pub(crate) fn atom_of(program: &ResidualProgram, text: &str) -> u32 {
    program.atoms.iter().position(|atom| {
        let atom: &str = atom.as_ref();
        atom == text
    }).expect("an atom with that name") as u32
}

/// The identity of every register `cell_at` answers for, in every function of `program`.
pub(crate) fn identities_of(program: &ResidualProgram, facts: &AotFacts) -> Vec<Identity> {
    let mut found = Vec::new();
    for id in 0..program.function_count() {
        let (len, registers) = program.with_record(id, |function| (function.code.len() as u32, usize::from(function.registers)));
        let function = facts.function_facts(id as u32);
        for pc in 0..len {
            for register in 0..registers {
                if let Some(cell) = function.cell_at(pc, register as u16) { found.push(cell.identity); }
            }
        }
    }
    found
}

/// The pc of function `name`'s last instruction (its return) and its register count.
pub(crate) fn last_pc_and_registers(program: &ResidualProgram, name: &str) -> (u32, usize) {
    let id = function_named(program, name);
    program.with_record(id, |function| (function.code.len() as u32 - 1, usize::from(function.registers)))
}

/// The identity of every register `cell_at` answers for in the function named `name`.
pub(crate) fn identities_in(program: &ResidualProgram, facts: &AotFacts, name: &str) -> Vec<Identity> {
    let id = function_named(program, name);
    let (len, registers) = program.with_record(id, |function| (function.code.len() as u32, usize::from(function.registers)));
    let function = facts.function_facts(id as u32);
    let mut found = Vec::new();
    for pc in 0..len {
        for register in 0..registers {
            if let Some(cell) = function.cell_at(pc, register as u16) { found.push(cell.identity); }
        }
    }
    found
}

/// The call targets the calls of function `function` name, in the closed solve.
pub(crate) fn targets_in(facts: &AotFacts, function: u32) -> Vec<u32> {
    facts.call_targets.iter().filter(|((caller, _), _)| *caller == function).map(|(_, &target)| target).collect()
}

/// The builtin path of global `global`, or of its member `member`, the program names.
pub(crate) fn builtin_path(program: &ResidualProgram, global: &str, member: Option<&str>) -> BuiltinPath {
    BuiltinPath { global: atom_of(program, global), member: member.map(|name| atom_of(program, name)) }
}

/// The type the function named `f` returns in the closed solve of `source`.
pub(crate) fn returned_type(source: &str) -> Type {
    let program = program(source);
    let facts = analyze(&program);
    facts.function_facts(function_named(&program, "f") as u32).return_cell().ty
}
