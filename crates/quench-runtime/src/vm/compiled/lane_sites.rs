//! What a lane unit's by-name loads read at compile time: the global scope's bindings.
use super::*;
use super::super::global_facts::GlobalRead;

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::lane_load_name_first`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns; each with its constant
    /// where the name is an immutable global (`builtins::immutable_globals`), else with the
    /// cell it reads now as a Stable fact (`Vm::stable_global_read`).
    pub(in crate::vm) fn lane_global_reads(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit)
        -> crate::vm::global_facts::GlobalReads {
        let global = |function: u32| program.function_get(function as usize)
            .is_some_and(|body| body.names_resolve_statically(program))
            && self.programs.names_are_global(self.active_program);
        let reads: Vec<(usize, GlobalRead)> = unit.instructions.iter().enumerate()
            .filter(|(_, instruction)| instruction.op().is_throwing_name_load())
            .filter(|(pc, instruction)| {
                let atom = instruction.atom_index();
                let function = match unit.origins[*pc] {
                    lane_unit::Origin::Caller(_) => function,
                    lane_unit::Origin::Inlined { frame, .. } => unit.frames[frame].target.function,
                };
                global(function) && (!self.atom_class(atom).contains(super::super::object::AtomClass::INTERNAL)
                    || crate::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&self.atom_name(atom)))
            })
            .map(|(pc, instruction)| (pc, instruction.atom_index(), instruction.cache_site_index()))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(pc, atom, site)| {
                let name = self.atom_name(atom);
                let immutable = super::super::builtins::immutable_globals().into_iter().find(|(global, _)| *global == name);
                (pc, immutable.map_or_else(|| self.stable_global_read(program, atom, site), |(_, value)| GlobalRead::Constant(value)))
            })
            .collect();
        let members: Vec<(usize, GlobalRead)> = reads.iter().filter_map(|(pc, read)| self.native_member(unit, *pc, read)).collect();
        let methods = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)| Some((pc, self.method_native(instruction)?)));
        let methods: Vec<(usize, GlobalRead)> = methods.collect();
        reads.into_iter().chain(members).chain(methods).collect()
    }

    /// The field read `instruction` of a method of strings or arrays that a call runs inline
    /// (`lane_region::native_calls`), where the realm's prototype holds that native as an own
    /// data property now. The receiver is not known: the call guards the callee it reads.
    fn method_native(&self, instruction: &crate::bytecode::WideInstruction) -> Option<GlobalRead> {
        /// The methods a call runs inline, by name.
        const METHODS: [(&str, crate::vm::Native); 4] = [("charCodeAt", crate::vm::Native::StringCharCodeAt), ("charAt", crate::vm::Native::StringCharAt),
            ("push", crate::vm::Native::ArrayPush), ("pop", crate::vm::Native::ArrayPop)];
        if !instruction.op().is_get_field() { return None; }
        let Some(crate::bytecode::FieldLookup::Atom { atom, .. }) = instruction.field_lookup() else { return None };
        let name = self.atom_name(atom);
        let (_, native) = METHODS.iter().find(|(method, _)| *method == name)?;
        let holder = if matches!(native, crate::vm::Native::ArrayPush | crate::vm::Native::ArrayPop) { self.array_proto } else { self.string_proto };
        let value = self.own_plain_data(holder, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(held), .. } = self.heap.function(value)? else { return None };
        (held == native).then_some(GlobalRead::NativeMember { value, native: *native })
    }

    /// The field read that follows the global read `read` at `pc` and reads its register, where
    /// the member it names is an own data property holding a native function now.
    fn native_member(&self, unit: &lane_unit::Unit, pc: usize, read: &GlobalRead) -> Option<(usize, GlobalRead)> {
        let (GlobalRead::Constant(object) | GlobalRead::Stable { value: object, .. }) = *read else { return None };
        let (base, member) = (unit.instructions.get(pc)?.result_register(), unit.instructions.get(pc + 1)?);
        if !member.op().is_get_field() { return None; }
        let Some(crate::bytecode::FieldLookup::Atom { base: read_base, atom, .. }) = member.field_lookup() else { return None };
        if read_base.register_index() != Some(base) { return None; }
        let value = self.own_plain_data(object, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(native), .. } = self.heap.function(value)? else { return None };
        Some((pc + 1, GlobalRead::NativeMember { value, native: *native }))
    }
}
