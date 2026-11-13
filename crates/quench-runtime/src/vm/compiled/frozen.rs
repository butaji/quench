//! Whole-program facts a frozen image folds: the globals and prototype fields the program
//! writes exactly once, with an immediate, before anything can observe them. Atoms are dense
//! ids, so each table is a flat vector indexed by atom.

use super::*;
use crate::bitset::BitSet;

/// How many stores each atom has across the program's by-name stores.
fn global_store_counts(program: &ResidualProgram) -> crate::dense_map::DenseMap<usize> {
    let mut stores = crate::dense_map::DenseMap::<usize>::default();
    for id in 0..program.function_count() {
        let function = program.function(id);
        for &packed in function.code.iter() {
            let instruction = super::dispatch_fast::decoded_instruction(function, packed);
            if instruction.op().stores_name() {
                *stores.entry_or_default(instruction.atom_index()) += 1;
            }
        }
    }
    stores
}

fn root_immediate_globals(program: &ResidualProgram, candidates: &BitSet,
    stores: &crate::dense_map::DenseMap<usize>,
) -> crate::dense_map::DenseMap<Value> {
    let root = program.function(0);
    let mut values = crate::dense_map::DenseMap::default();
    let mut immediates = crate::dense_map::DenseMap::<Value>::default();
    for &packed in root.code.iter() {
        let instruction = super::dispatch_fast::decoded_instruction(root, packed);
        for register in instruction.written_registers() { immediates.remove(u32::from(register)); }
        if instruction.op().js_denotation() == crate::bytecode::JsDenotation::LoadConst {
            if let Some(value) = program.constants.get(instruction.constant_index()).and_then(facts::immediate) {
                immediates.insert(u32::from(instruction.result_register()), value);
            }
        } else if instruction.op().stores_name() && instruction.b() != 0 {
            // The declaration's own store (`initializing`) carries its initializer.
            let atom = instruction.atom_index();
            if candidates.contains(atom as usize) && stores.get(&atom) == Some(&1)
                && let Some(&value) = immediates.get(&u32::from(instruction.register_a())) {
                values.insert(atom, value);
            }
        }
    }
    values
}

/// Whole-program globals a frozen image may fold: a `var` the program writes exactly
/// once, with an immediate, before any other function can observe it.
pub(in crate::vm) fn frozen_globals(program: &ResidualProgram) -> crate::dense_map::DenseMap<Value> {
    let root = program.function(0);
    let mut candidates = BitSet::default();
    root.global_var_atoms().iter().for_each(|&atom| {
        candidates.insert_growing(atom as usize);
    });
    let stores = global_store_counts(program);
    root_immediate_globals(program, &candidates, &stores)
}

fn field_store_counts(program: &ResidualProgram) -> crate::dense_map::DenseMap<usize> {
    let mut counts = crate::dense_map::DenseMap::<usize>::default();
    for id in 0..program.function_count() {
        let function = program.function(id);
        for &packed in function.code.iter() {
            let instruction = super::dispatch_fast::decoded_instruction(function, packed);
            if instruction.op().js_denotation() == crate::bytecode::JsDenotation::FieldStore {
                *counts.entry_or_default(instruction.atom_index()) += 1;
            }
        }
    }
    counts
}

fn root_immediate_fields(program: &ResidualProgram,
    counts: &crate::dense_map::DenseMap<usize>,
) -> crate::dense_map::DenseMap<Value> {
    let root = program.function(0);
    let mut immediates = crate::dense_map::DenseMap::<Value>::default();
    let mut values = crate::dense_map::DenseMap::default();
    for &packed in root.code.iter() {
        let instruction = super::dispatch_fast::decoded_instruction(root, packed);
        for register in instruction.written_registers() { immediates.remove(u32::from(register)); }
        if instruction.op().js_denotation() == crate::bytecode::JsDenotation::LoadConst {
            if let Some(value) = program.constants.get(instruction.constant_index()).and_then(facts::immediate) {
                immediates.insert(u32::from(instruction.result_register()), value);
            }
        } else if instruction.op().js_denotation() == crate::bytecode::JsDenotation::FieldStore {
            let atom = instruction.atom_index();
            if counts.get(&atom) == Some(&1)
                && let Some(&value) = immediates.get(&u32::from(instruction.register_a())) {
                values.insert(atom, value);
            }
        }
    }
    values
}

/// Whole-program prototype values: a field the root stores exactly once with an
/// immediate (a constructor's `prototype.k = <immediate>`), folded at each read and
/// guarded there. Accessor definitions are outside this analysis.
pub(in crate::vm) fn frozen_fields(program: &ResidualProgram) -> crate::dense_map::DenseMap<Value> {
    let counts = field_store_counts(program);
    root_immediate_fields(program, &counts)
}

/// Each by-name load or capture read of a frozen global, with the closure's whole-program
/// value. A capture read names the binding through the environment slot it closes over, so
/// its owner's local atom decides whether it reads a frozen global; the value it actually
/// loads is compared with the frozen one at the read site, which keeps the fold sound even
/// for a read before the declaration.
pub(in crate::vm) fn frozen_global_reads(program: &ResidualProgram, function: &Function,
    globals: &crate::dense_map::DenseMap<Value>,
) -> crate::vm::global_facts::GlobalReads {
    let frozen = |atom: Atom| globals.get(&atom).map(|&value|
        crate::vm::global_facts::GlobalRead::Frozen { value, ty: crate::typed_ir::Type::immediate(value) });
    function.code.iter().enumerate().filter_map(|(pc, &packed)| {
        let instruction = super::dispatch_fast::decoded_instruction(function, packed);
        let read = if instruction.op().loads_name() {
            frozen(instruction.atom_index())
        } else if instruction.op().addresses_capture() {
            capture_binding_atom(program, function, instruction).and_then(frozen)
        } else {
            None
        };
        read.map(|read| (pc, read))
    }).collect()
}

/// The atom a capture read names: the local atom of the slot its environment closes over.
fn capture_binding_atom(program: &ResidualProgram, function: &Function,
    instruction: crate::bytecode::WideInstruction,
) -> Option<Atom> {
    let mut owner = function.parent?;
    for _ in 0..crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()) {
        owner = program.function(owner as usize).parent?;
    }
    let slot = usize::from(crate::bytecode::ImmediateLayout::capture_slot(instruction.imm()));
    program.function(owner as usize).local_atoms.get(slot).copied()
}

