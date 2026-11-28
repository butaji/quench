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
