//! Whole-program facts a frozen image folds: the globals and prototype fields the program
//! writes exactly once, with an immediate, before anything can observe them. Atoms are dense
//! ids, so each table is a flat vector indexed by atom.

use super::*;
use crate::bitset::BitSet;

type FrozenValues = crate::dense_map::DenseMap<Value>;

/// How many instructions of the program, over all functions, `counted` gives each atom.
fn atom_store_counts(program: &ResidualProgram, counted: impl Fn(WideInstruction) -> bool) -> Vec<u32> {
    let mut counts = vec![0_u32; program.atoms.len()];
    for id in 0..program.function_count() {
        let function = program.function(id);
        for &packed in function.code.iter() {
            let instruction = super::dispatch_fast::decoded_instruction(function, packed);
            if counted(instruction) && let Some(count) = counts.get_mut(instruction.atom_index() as usize) {
                *count += 1;
            }
        }
    }
    counts
}

/// The root's stores `stored` names an atom for, whose source register holds an immediate
/// the root loaded, as (atom, value) in program order.
fn root_immediate_stores(program: &ResidualProgram, stored: impl Fn(WideInstruction) -> Option<Atom>) -> FrozenValues {
    let root = program.function(0);
    let mut values = FrozenValues::default();
    let mut immediates: Vec<Option<Value>> = vec![None; usize::from(root.registers)];
    for &packed in root.code.iter() {
        let instruction = super::dispatch_fast::decoded_instruction(root, packed);
        for register in instruction.written_registers() {
            if let Some(slot) = immediates.get_mut(usize::from(register)) { *slot = None; }
        }
        if instruction.op().js_denotation() == crate::bytecode::JsDenotation::LoadConst {
            let value = program.constants.get(instruction.constant_index()).and_then(facts::immediate);
            if let Some(slot) = immediates.get_mut(usize::from(instruction.result_register())) { *slot = value; }
        } else if let Some(atom) = stored(instruction)
            && let Some(Some(value)) = immediates.get(usize::from(instruction.register_a())) {
            values.insert(atom, *value);
        }
    }
    values
}

/// Whole-program globals a frozen image may fold: a `var` the program writes exactly
/// once, with an immediate, before any other function can observe it.
pub(in crate::vm) fn frozen_globals(program: &ResidualProgram) -> FrozenValues {
    let mut candidates = BitSet::default();
    program.function(0).global_var_atoms().iter().for_each(|&atom| {
        candidates.insert_growing(atom as usize);
    });
    let stores = atom_store_counts(program, |instruction| instruction.op().stores_name());
    // The declaration's own store (`initializing`) carries its initializer.
    root_immediate_stores(program, |instruction| {
        let atom = instruction.atom_index();
        (instruction.op().stores_name() && instruction.b() != 0 && candidates.contains(atom as usize)
            && stores.get(atom as usize) == Some(&1)).then_some(atom)
    })
}

/// Whole-program prototype values: a field the root stores exactly once with an
/// immediate (a constructor's `prototype.k = <immediate>`), folded at each read and
/// guarded there. Accessor definitions are outside this analysis.
pub(in crate::vm) fn frozen_fields(program: &ResidualProgram) -> FrozenValues {
    let field_store = |instruction: WideInstruction| instruction.op().js_denotation() == crate::bytecode::JsDenotation::FieldStore;
    let counts = atom_store_counts(program, field_store);
    root_immediate_stores(program, |instruction| {
        let atom = instruction.atom_index();
        (field_store(instruction) && counts.get(atom as usize) == Some(&1)).then_some(atom)
    })
}

/// Each by-name load or capture read of a frozen global, with the closure's whole-program
/// value. A capture read names the binding through the environment slot it closes over, so
/// its owner's local atom decides whether it reads a frozen global; the value it actually
/// loads is compared with the frozen one at the read site, which keeps the fold sound even
/// for a read before the declaration.
pub(in crate::vm) fn frozen_global_reads(program: &ResidualProgram, function: &Function,
    globals: &FrozenValues,
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

