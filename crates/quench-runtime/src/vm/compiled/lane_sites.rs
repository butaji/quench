//! What a lane unit's read sites name at compile time: the global scope's bindings its
//! by-name loads read, and the field caches its field reads specialize on.
use super::*;

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::lane_load_name_first`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns; each with its constant
    /// where the name is an immutable global (`builtins::immutable_globals`).
    pub(super) fn lane_global_reads(&self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> std::collections::BTreeMap<usize, Option<Value>> {
        let global = |function: u32| program.function_get(function as usize)
            .is_some_and(|body| body.names_resolve_statically(program))
            && self.programs.unresolved_names_are_global(self.active_program, function);
        unit.instructions.iter().enumerate()
            .filter(|(_, instruction)| matches!(instruction.op(), Op::LoadName | Op::LoadNameCall))
            .filter(|(pc, instruction)| {
                let atom = instruction.atom_index();
                let function = match unit.origins[*pc] {
                    lane_unit::Origin::Caller(_) => function,
                    lane_unit::Origin::Inlined { frame, .. } => unit.frames[frame].target.function,
                };
                global(function) && (!self.atom_class(atom).contains(super::super::object::AtomClass::INTERNAL)
                    || crate::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&self.atom_name(atom)))
            })
            .map(|(pc, instruction)| {
                let name = self.atom_name(instruction.atom_index());
                (pc, super::super::builtins::immutable_globals().into_iter().find(|(global, _)| *global == name).map(|(_, value)| value))
            })
            .collect()
    }

    /// The field cache indices `unit`'s reads specialize on (`LaneMethod::read_sites`), which
    /// its publication subscribes to (`ProgramStore::field_site_readers`).
    pub(super) fn lane_read_sites(&self, program: &ResidualProgram, unit: &lane_unit::Unit) -> Box<[u32]> {
        unit.instructions.iter()
            .flat_map(|instruction| Self::instruction_field_sites(program, instruction))
            .filter(|(_, read)| *read).map(|(site, _)| self.field_cache_index(site) as u32)
            .collect::<std::collections::BTreeSet<_>>().into_iter().collect()
    }
}
