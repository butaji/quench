//! Tag and shape tests the solver type already decides. A register whose solver type is a subtype of a tag's type holds that
//! tag at every point it reaches, so a site that would test the tag emits no test for it (the known-types law): a
//! test is made only where the type cannot prove the tag. A direct object's solver type proves its family (a non-nullable
//! object record, never a function), so the family and presence tests on it are redundant too.
use super::*;
use quench_runtime::typed_ir::{BuiltinPath, Identity};

/// Whether `instruction` loads a binding of the global scope by name: the solver's `is_global_load`, the name a builtin path
/// is named by (`named_builtin`).
fn loads_global_name(instruction: WideInstruction) -> bool {
    instruction.op().loads_name() && instruction.op().traits().has(quench_runtime::bytecode::Traits::GLOBAL_NAME_ACCESS)
}

/// Whether register `register` holds `tag`'s language types at this point, by its solver type in `state`. A tag site
/// asks this before it emits its tag test.
pub(crate) fn register_proves(state: &State, register: Register, tag: Type) -> bool {
    state.values[usize::from(register)].value.ty.is_subtype(tag)
}

/// Whether a value of solver type `ty` is a direct object's record (`Type::is_direct_object_family`): its record is present
/// and it is not a function, so a test that it is an object, or that its record is present, is redundant.
pub(crate) fn proves_object(ty: Type) -> bool {
    ty.is_direct_object_family()
}

/// Whether every string of solver type `ty` has a length int32 holds (`StringFact::length`), so a flat string of that
/// type needs no test that its length fits: its record's length lies in the type's range. A type with no string fact
/// (or the full length range) proves nothing.
pub(crate) fn length_fits_int32(ty: Type) -> bool {
    ty.string_fact().is_some_and(|fact| fact.length().max <= i32::MAX as u32)
}

/// Whether the lane's own type of `register` in `state` is a flat string (`StringFact::flat`): the lowering's
/// known-types law, for a site with no unit PC to ask the solve at (`proven_flat` is the solve's answer).
pub(crate) fn state_flat(state: &State, register: Register) -> bool {
    state.values[usize::from(register)].value.ty.string_fact().is_some_and(|fact| fact.flat())
}

/// Whether `comparison_decision` answers a Boolean, never `DELETED`, for two flat strings under `operator`: the equality
/// operators (`identity_equality` and `strict_equal` compare two strings by their contents) and the relational operators
/// (`string_relation` orders them). `in` and `instanceof` are not: they leave a string pair to the full step.
pub(crate) fn decides_flat_strings(operator: u32) -> bool {
    quench_runtime::vm::operations::equality_operator(operator).is_some()
        || quench_runtime::vm::operations::NumberRelation::from_operator(operator)
            .is_some_and(|relation| relation.of_ordering(std::cmp::Ordering::Less).is_some())
}

impl Lowering<'_, '_> {
    /// Whether the solve proves unit register `register` holds the builtin `expected` just before the unit instruction at `pc`
    /// (`Identity::Builtin(expected)`, proven in every run by the closed world). A site that would compare the register's
    /// identity with that builtin's value emits no compare. Some other builtin does not prove it.
    pub(crate) fn proves_builtin(&self, pc: usize, register: Register, expected: BuiltinPath) -> bool {
        self.cell_before(pc, register).identity == Identity::Builtin(expected)
    }

    /// The builtin the value the unit instruction at `defined` reads by name stands for, named from the program's own atoms
    /// in the solver's naming (`BuiltinPath`, `aot_facts::effects`): a name load of global `g` is `{g, None}`, and a field
    /// read `m` off the register a name load of `g` defined is `{g, Some(m)}`. None where the read names no builtin so.
    /// A wrong name only keeps a guard: `proves_builtin` must match the solve's identity exactly.
    pub(crate) fn named_builtin(&self, defined: usize) -> Option<BuiltinPath> {
        let instruction = self.unit.instructions[defined];
        if loads_global_name(instruction) {
            return Some(BuiltinPath { global: instruction.atom_index(), member: None });
        }
        let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup() else { return None };
        let owner = super::native_calls::definition_before(self.unit, defined, base.register_index()?)?;
        let global = self.unit.instructions[owner];
        loads_global_name(global).then(|| BuiltinPath { global: global.atom_index(), member: Some(atom) })
    }

    /// Whether the solve proves unit register `register` holds a direct object's record just before the unit instruction at
    /// `pc`, by its solver type (the known-types law): a family or presence test on it is redundant.
    pub(crate) fn solver_object(&self, pc: usize, register: Register) -> bool {
        proves_object(self.cell_before(pc, register).ty)
    }

    /// The settled type of field `atom` of the fixed class `owner` holds (`AotFacts::slot_domain`), where the class holds the
    /// field before any read (`field_always_stored`): every read of that slot yields a value of the type. None otherwise.
    pub(crate) fn fixed_slot_domain(&self, pc: usize, owner: Option<Register>, atom: u32) -> Option<Type> {
        let facts = self.vm.facts?;
        let class = self.fixed_class(pc, owner)?;
        if !facts.field_always_stored(class, atom) { return None; }
        facts.slot_domain(class, atom).map(|cell| cell.ty)
    }

    /// Whether the value `register` holds after the unit instruction at `pc` is the one closure of body `body`
    /// (`Identity::Closure`, proven in every run by the closed world): its identity guard is not emitted.
    pub(crate) fn is_body_closure_identity(&self, pc: usize, register: Register, body: u32) -> bool {
        self.cell_after(pc, register).identity == quench_runtime::typed_ir::Identity::Closure(body)
    }

    /// Whether unit register `register` holds a flat string just before the unit instruction at `unit_pc`, as the solve
    /// proves it (`FunctionFacts::string_flat`). No facts, or no origin: false.
    pub(crate) fn proven_flat(&self, unit_pc: usize, register: Register) -> bool {
        let Some(facts) = self.vm.facts else { return false };
        let Some((function, pc, local)) = self.origin_local(unit_pc, register) else { return false };
        facts.function_facts(function).string_flat(pc, local)
    }

    /// Whether unit register `register` holds a plain int32 backing just before the unit instruction at `unit_pc`, as the
    /// solve proves it (`FunctionFacts::backing_plain`), whose count of other values is zero. No facts, or no origin: false.
    pub(crate) fn proven_plain_backing(&self, unit_pc: usize, register: Register) -> bool {
        let Some(facts) = self.vm.facts else { return false };
        let Some((function, pc, local)) = self.origin_local(unit_pc, register) else { return false };
        facts.function_facts(function).backing_plain(pc, local)
    }

    /// Whether an unwritten element of a frame array reads as `undefined`: no builtin prototype object has an index property
    /// in the closed world the facts describe (`AotFacts::prototype_indices_empty`), an edge fact with no run-time check.
    /// Without facts, it is not known.
    pub(crate) fn holes_read_undefined(&self) -> bool {
        self.vm.facts.is_some_and(|facts| facts.function_facts(self.function_id).prototype_indices_empty())
    }

    /// The fixed class (`AotFacts::class_fixed_cell`) every instance of which the receiver `owner` holds just before the unit
    /// instruction at `pc`: the register's cell (`FunctionFacts::cell_at`) names that one class, proven; `None` names `this`,
    /// whose class is the function's `this_classes`, the unit's own function only (an inlined frame's `this` is its
    /// receiver register). None where the solve proves no single class, or the class is not fixed.
    pub(crate) fn fixed_class(&self, pc: usize, owner: Option<Register>) -> Option<u32> {
        use super::super::lane_unit::Origin;
        let facts = self.vm.facts?;
        let classes = match owner {
            Some(register) => {
                let (function, origin_pc, local) = self.origin_local(pc, register)?;
                facts.function_facts(function).cell_at(origin_pc, local)?.classes
            }
            None => match self.unit.origins.get(pc)? {
                Origin::Caller(_) => facts.function_facts(self.function_id).this_classes(),
                Origin::Inlined { .. } => return None,
            },
        };
        proven_class(classes).filter(|&class| facts.class_fixed_cell(class) && facts.spill_from(class).is_none())
    }

    /// The fixed class (`fixed_class`) whose instances the lane may store into in place: its cell is no flag transition
    /// reaches (`AotFacts::class_writable_cell`), so a store's slot is never frozen out from under it. A store site that
    /// addresses a fixed cell uses this; a read uses `fixed_class`.
    pub(crate) fn writable_class(&self, pc: usize, owner: Option<Register>) -> Option<u32> {
        let class = self.fixed_class(pc, owner)?;
        let facts = self.vm.facts?;
        facts.class_writable_cell(class).then_some(class)
    }
}

/// The one class `classes` proves its objects are instances of: a construction's own result (`proven`), of a single class.
/// A class the solve only guessed, or several, proves nothing.
pub(crate) fn proven_class(classes: quench_runtime::typed_ir::Classes) -> Option<u32> {
    match classes {
        quench_runtime::typed_ir::Classes::Set { ids, len: 1, proven: true } => Some(ids[0]),
        _ => None,
    }
}

/// The slot of field `atom` in the union layout of `class` (`AotFacts::class_union_layout`): the slot a receiver of the class
/// built by its constructor holds the name in (`Vm::seed_union_domains` seeds that slot's domain).
pub(crate) fn union_slot(facts: &quench_runtime::compile::aot_facts::AotFacts, class: u32, atom: u32) -> Option<usize> {
    facts.class_union_layout(class)?.fields.iter().position(|(name, _)| *name == atom)
}

/// Whether a store of `writes` to field `atom` of an instance of `class` needs no run-time admission: the field's settled domain
/// (`AotFacts::slot_domain`) is not Tagged and holds the writes. The domain is seeded on the slot (`Vm::seed_union_domains`), so the
/// admission would only confirm it. A field the class's union layout does not name is not proven.
pub(crate) fn slot_write_seeded(facts: &quench_runtime::compile::aot_facts::AotFacts, class: u32, atom: u32, writes: Type) -> bool {
    let Some(domain) = facts.slot_domain(class, atom) else { return false };
    quench_runtime::typed_ir::SlotType::from_type(domain.ty) != quench_runtime::typed_ir::SlotType::Tagged && writes.is_subtype(domain.ty)
}

/// Whether the store `instruction` at unit pc `unit_pc` of `unit` (whose caller function is `function`) needs no run-time admission
/// of `writes`: the receiver register it names holds, before the store, the proven single class of its solve
/// (`FunctionFacts::cell_at`), and `slot_write_seeded` holds for that class and the store's atom. The receiver of an inlined frame's
/// store with no register of its own is the frame's receiver. Otherwise the run-time admission is the slow path.
pub(crate) fn store_seeded(facts: &quench_runtime::compile::aot_facts::AotFacts, unit: &super::super::lane_unit::Unit, function: u32,
    unit_pc: usize, instruction: WideInstruction, writes: Type) -> bool
{
    use super::super::lane_unit::Origin;
    let receiver = match (instruction.field_store_register(), unit.origins.get(unit_pc)) {
        (Some(register), Some(_)) => register,
        (None, Some(Origin::Inlined { frame, .. })) => unit.frames[*frame].receiver,
        _ => return false,
    };
    let (function, pc, local) = match unit.origins[unit_pc] {
        Origin::Caller(pc) => (function, pc, receiver),
        Origin::Inlined { frame, pc } => {
            let frame = &unit.frames[frame];
            let Some(local) = receiver.checked_sub(frame.base) else { return false };
            (frame.target.function, pc, local)
        }
    };
    let Some(cell) = facts.function_facts(function).cell_at(pc, local) else { return false };
    proven_class(cell.classes).is_some_and(|class| slot_write_seeded(facts, class, instruction.atom_index(), writes))
}
