//! Whole-program facts a frozen image folds: globals and prototype fields the program
//! writes exactly once, with an immediate, in its root body.
use super::*;

/// Values a frozen image folds, by atom: a flat table holding `Value::DELETED` for an atom
/// with no folded value.
pub(in crate::vm) struct FrozenValues {
    values: Box<[Value]>,
}

impl FrozenValues {
    /// The folded value of `atom`, if any.
    pub(in crate::vm) fn get(&self, atom: &Atom) -> Option<&Value> {
        self.values.get(*atom as usize).filter(|value| !value.is_deleted())
    }

    /// The atoms with a folded value, in order.
    #[cfg(feature = "aot")]
    pub(in crate::vm) fn atoms(&self) -> impl Iterator<Item = Atom> + '_ {
        self.values.iter().enumerate().filter(|(_, value)| !value.is_deleted()).map(|(atom, _)| atom as Atom)
    }

    #[cfg(feature = "aot")]
    pub(in crate::vm) fn len(&self) -> usize {
        self.atoms().count()
    }
}

#[cfg(feature = "aot")]
/// How many instructions `writes` selects store each atom, across the program, by atom.
fn store_counts(program: &ResidualProgram, writes: impl Fn(WideInstruction) -> bool) -> Box<[u32]> {
    let mut counts = vec![0_u32; program.atoms.len()].into_boxed_slice();
    for id in 0..program.function_count() {
        let function = program.function(id);
        for &packed in function.code.iter() {
            let instruction = super::dispatch_fast::decoded_instruction(function, packed);
            if writes(instruction) && let Some(count) = counts.get_mut(instruction.atom_index() as usize) {
                *count = count.saturating_add(1);
            }
        }
    }
    counts
}

#[cfg(feature = "aot")]
/// The immediates the root body stores through the instructions `stores` selects, of the
/// atoms `admits` takes: the value a constant load left in the stored register, by atom.
fn root_immediate_stores(program: &ResidualProgram, stores: impl Fn(WideInstruction) -> bool,
    admits: impl Fn(Atom) -> bool) -> FrozenValues {
    let root = program.function(0);
    let mut immediates: Box<[Option<Value>]> = vec![None; usize::from(root.registers)].into_boxed_slice();
    let mut values = vec![Value::DELETED; program.atoms.len()].into_boxed_slice();
    for &packed in root.code.iter() {
        let instruction = super::dispatch_fast::decoded_instruction(root, packed);
        for register in instruction.written_registers() {
            if let Some(slot) = immediates.get_mut(usize::from(register)) { *slot = None; }
        }
        if instruction.op().js_denotation() == crate::bytecode::JsDenotation::LoadConst {
            let value = program.constants.get(instruction.constant_index()).and_then(facts::immediate);
            if let Some(slot) = immediates.get_mut(usize::from(instruction.result_register())) { *slot = value; }
        } else if stores(instruction) && admits(instruction.atom_index())
            && let Some(&Some(value)) = immediates.get(usize::from(instruction.register_a())) {
            values[instruction.atom_index() as usize] = value;
        }
    }
    FrozenValues { values }
}

#[cfg(feature = "aot")]
/// Whole-program globals a frozen image may fold: a `var` the program writes exactly
/// once, with an immediate, before any other function can observe it. The declaration's
/// own store (`initializing`, its field B) carries its initializer.
pub(in crate::vm) fn frozen_globals(program: &ResidualProgram) -> FrozenValues {
    let mut candidates = crate::vm::AtomSet::default();
    for &atom in program.function(0).global_var_atoms() { candidates.insert(atom); }
    let counts = store_counts(program, |instruction| instruction.op().stores_name());
    root_immediate_stores(program, |instruction| instruction.op().stores_name() && instruction.b() != 0,
        |atom| candidates.contains(&atom) && counts[atom as usize] == 1)
}

#[cfg(feature = "aot")]
/// Whole-program prototype values: a field the root stores exactly once with an
/// immediate (a constructor's `prototype.k = <immediate>`), folded at each read and
/// guarded there. Accessor definitions are outside this analysis.
pub(in crate::vm) fn frozen_fields(program: &ResidualProgram) -> FrozenValues {
    let field_store = |instruction: WideInstruction| instruction.op().js_denotation() == crate::bytecode::JsDenotation::FieldStore;
    let counts = store_counts(program, field_store);
    root_immediate_stores(program, field_store, |atom| counts[atom as usize] == 1)
}

#[cfg(feature = "aot")]
/// Each by-name load of a frozen global, with the closure's whole-program value.
pub(in crate::vm) fn frozen_global_reads(function: &Function, globals: &FrozenValues)
    -> crate::vm::global_facts::GlobalReads {
    function.code.iter().enumerate().filter_map(|(pc, &packed)| {
        let instruction = super::dispatch_fast::decoded_instruction(function, packed);
        if !instruction.op().loads_name() { return None; }
        globals.get(&instruction.atom_index()).map(|&value|
            (pc, crate::vm::global_facts::GlobalRead::Frozen { value, ty: crate::typed_ir::Type::immediate(value) }))
    }).collect()
}
