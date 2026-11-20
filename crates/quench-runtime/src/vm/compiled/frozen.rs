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
            let instruction = function.widened(packed);
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
        let instruction = root.widened(packed);
        for register in instruction.written_registers() {
            if let Some(slot) = immediates.get_mut(usize::from(register)) { *slot = None; }
        }
        if let Some((destination, constant)) = crate::staged::Operation::<(), ()>::decode_constant_load(instruction) {
            let value = program.constants.get(constant).and_then(facts::immediate);
            if let Some(slot) = immediates.get_mut(usize::from(destination)) { *slot = value; }
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
        // `atom_index` is a by-name op's field: read it only where the op stores a name.
        if !instruction.op().stores_name() { return None }
        let atom = instruction.atom_index();
        (instruction.b() != 0 && candidates.contains(atom as usize)
            && stores.get(atom as usize) == Some(&1)).then_some(atom)
    })
}

/// Whole-program prototype values: a field the root stores exactly once with an
/// immediate (a constructor's `prototype.k = <immediate>`), folded at each read and
/// guarded there. Accessor definitions are outside this analysis.
pub(in crate::vm) fn frozen_fields(program: &ResidualProgram) -> FrozenValues {
    // A store to a register receiver (`SetField`, `SetFieldStrict`); a store to `this` is not one.
    let field_store = |instruction: WideInstruction| matches!(crate::vm::compiled::decoded(instruction),
        Some(crate::staged::Operation::SetField { base: Some(_), .. }));
    let counts = atom_store_counts(program, field_store);
    root_immediate_stores(program, |instruction| {
        if !field_store(instruction) { return None }
        let atom = instruction.atom_index();
        (counts.get(atom as usize) == Some(&1)).then_some(atom)
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
    function.code.iter().enumerate().filter_map(|(pc, &packed)| {
        frozen_read(program, function, function.widened(packed), globals).map(|read| (pc, read))
    }).collect()
}

/// `frozen_global_reads` over a lane unit, by unit PC: each instruction read in the function
/// its origin names (the caller's, or an inlined frame's callee).
pub(in crate::vm) fn frozen_unit_reads(program: &ResidualProgram, unit: &super::lane_unit::Unit, function: u32,
    globals: &FrozenValues,
) -> crate::vm::global_facts::GlobalReads {
    unit.instructions.iter().enumerate().filter_map(|(pc, &instruction)| {
        let owner = match *unit.origins.get(pc)? {
            super::lane_unit::Origin::Caller(_) => function,
            super::lane_unit::Origin::Inlined { frame, .. } => unit.frames.get(frame)?.target.function,
        };
        let read = frozen_read(program, program.function_get(owner as usize)?, instruction, globals)?;
        Some((pc, read))
    }).collect()
}

/// The frozen read `instruction` of `function` makes: a by-name load or a capture access of a
/// global `globals` holds, with its whole-program value.
fn frozen_read(program: &ResidualProgram, function: &Function, instruction: WideInstruction,
    globals: &FrozenValues,
) -> Option<crate::vm::global_facts::GlobalRead> {
    let atom = if instruction.op().loads_name() {
        instruction.atom_index()
    } else if matches!(crate::vm::compiled::decoded_completed(instruction),
        Some(crate::staged::Operation::LoadCapture { .. } | crate::staged::Operation::StoreCapture { .. })) {
        capture_binding_atom(program, function, instruction)?
    } else {
        return None;
    };
    globals.get(&atom).map(|&value| crate::vm::global_facts::GlobalRead::Frozen { value, ty: crate::typed_ir::Type::immediate(value) })
}

/// The whole-program values a closed image's lane bodies fold, each checked at its read site
/// (`Division`-independent: a frozen value's binding time is Static, `staged::division`):
/// derived once per image, read by every function's lowering.
pub(in crate::vm) struct ClosedFrozen {
    /// The globals the program writes once with an immediate (`frozen_globals`).
    pub(in crate::vm) globals: FrozenValues,
    /// The prototype fields the program writes once with an immediate (`frozen_fields`).
    pub(in crate::vm) fields: FrozenValues,
}

impl ClosedFrozen {
    pub(in crate::vm) fn of(program: &ResidualProgram) -> Self {
        Self { globals: frozen_globals(program), fields: frozen_fields(program) }
    }
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

