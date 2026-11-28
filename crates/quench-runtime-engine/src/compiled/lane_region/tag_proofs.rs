//! Tag and shape tests the solver type already decides. A register whose solver type is a subtype of a tag's type holds that
//! tag at every point it reaches, so a site that would test the tag emits no test for it (the known-types law): a
//! test is made only where the type cannot prove the tag. A direct object's solver type proves its family (a non-nullable
//! object record, never a function), so the family and presence tests on it are redundant too.
use super::*;

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

impl Lowering<'_, '_> {
    /// Whether the unit's only write of `register` is the one at `defined`, and every run that reaches `pc` has passed
    /// `defined`: the register then holds the value that write defined at `pc`. The entries of the unit are its start, its
    /// resume point (`stable.start`), and nothing else, so a unit whose functions carry an exception handler is not proven:
    /// a handler's entry is no edge of the instruction stream.
    pub(crate) fn sole_definition_dominates(&self, defined: usize, pc: usize, register: Register) -> bool {
        let written_elsewhere = self.unit.instructions.iter().enumerate()
            .any(|(at, instruction)| at != defined && instruction.written_registers().any(|written| written == register));
        if written_elsewhere || self.unit_has_handlers() { return false; }
        let len = self.unit.instructions.len();
        let mut seen = vec![false; len];
        let mut pending: Vec<usize> = [0, self.stable.start].into_iter().filter(|&entry| entry < len && entry != defined).collect();
        while let Some(at) = pending.pop() {
            if at == pc { return false; }
            if std::mem::replace(&mut seen[at], true) { continue; }
            quench_runtime::bytecode::for_each_instruction_successor(at, self.unit.instructions[at], |next| {
                if next < len && next != defined && !seen[next] { pending.push(next); }
            });
        }
        true
    }

    /// Whether the function of the unit, or of any frame it inlines, carries an exception handler (`Function::handlers`).
    fn unit_has_handlers(&self) -> bool {
        std::iter::once(self.function_id).chain(self.unit.frames.iter().map(|frame| frame.target.function))
            .any(|function| self.program.function_get(function as usize).is_none_or(|body| (&body.handlers).into_iter().next().is_some()))
    }

    /// The builtin the value the unit instruction at `defined` defines names, where that definition is a by-name read of a
    /// global binding (`GlobalRead::NativeSlot`, `parseInt`) or a read of a member of one (`GlobalRead::NativeMember`, `Math.sqrt`,
    /// its receiver the global read the instruction before it defines). None for any other definition.
    pub(crate) fn builtin_definition(&self, defined: usize) -> Option<quench_runtime::compile::aot_facts::BuiltinPath> {
        use quench_runtime::vm::global_facts::GlobalRead;
        // The atom of the by-name load at `at`, where it reads the global scope.
        let global_name = |at: usize| -> Option<u32> {
            let instruction = *self.unit.instructions.get(at)?;
            matches!(quench_runtime::staged::Operation::<u64, ()>::decode_load_name(instruction, ()),
                Some(quench_runtime::staged::Operation::LoadName { .. })).then(|| instruction.atom_index())
        };
        match self.global_reads.get(&defined)? {
            GlobalRead::NativeSlot { .. } => Some(quench_runtime::compile::aot_facts::BuiltinPath { global: global_name(defined)?, member: None }),
            GlobalRead::NativeMember { folds: true, .. } => {
                let quench_runtime::bytecode::FieldLookup::Atom { atom, base, .. } = self.unit.instructions[defined].field_lookup()? else { return None };
                let read = defined.checked_sub(1)?;
                let base = base.register_index()?;
                if self.unit.instructions[read].result_register() != base { return None; }
                if !matches!(self.global_reads.get(&read), Some(GlobalRead::Constant(_) | GlobalRead::Stable { .. })) { return None; }
                if !self.sole_definition_dominates(read, defined, base) { return None; }
                Some(quench_runtime::compile::aot_facts::BuiltinPath { global: global_name(read)?, member: Some(atom) })
            }
            _ => None,
        }
    }

    /// Whether the call at unit PC `pc` calls the builtin its callee register `callee` was defined with at `defined`: that
    /// definition names a builtin the closed world proves is the realm's initial one (`AotFacts::builtin_identity`), and the
    /// definition is the register's value at the call. The call's identity guard is then not emitted.
    pub(crate) fn callee_is_builtin(&self, defined: usize, pc: usize, callee: Register) -> bool {
        let Some(facts) = self.vm.facts else { return false };
        self.builtin_definition(defined).is_some_and(|path| facts.builtin_identity(path))
            && self.sole_definition_dominates(defined, pc, callee)
    }

    /// Whether the value `register` holds after the unit instruction at `pc` is a closure of body `body`, the solve's type of
    /// it (`Type::function_body`), and the closed world makes every closure of `body` one object (`AotFacts::closure_identity`):
    /// the value is then that one object, so its identity guard is not emitted.
    pub(crate) fn is_body_closure_identity(&self, pc: usize, register: Register, body: u32) -> bool {
        let Some(facts) = self.vm.facts else { return false };
        facts.closure_identity(body) && self.proven_result(pc, register).and_then(|ty| ty.body_index()) == Some(body)
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
