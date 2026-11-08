//! What a lane unit's by-name loads read at compile time: the global scope's bindings.
use super::*;

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::lane_load_name_first`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns; each with its constant
    /// where the name is an immutable global (`builtins::immutable_globals`), or the global
    /// object where it is a fixed global variable (`Vm::fixed_global_variable`).
    pub(super) fn lane_global_reads(&self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> std::collections::BTreeMap<usize, lane_region::GlobalRead> {
        let global = |function: u32| program.function_get(function as usize)
            .is_some_and(|body| body.names_resolve_statically(program))
            && self.programs.unresolved_names_are_global(self.active_program, function);
        unit.instructions.iter().enumerate()
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
            .map(|(pc, instruction)| {
                let atom = instruction.atom_index();
                let name = self.atom_name(atom);
                let read = match super::super::builtins::immutable_globals().into_iter().find(|(global, _)| *global == name) {
                    Some((_, value)) => lane_region::GlobalRead::Constant(value),
                    None if self.fixed_global_variable(atom) => lane_region::GlobalRead::Fixed(self.realm.globals),
                    None => lane_region::GlobalRead::Dynamic,
                };
                (pc, read)
            })
            .collect()
    }
}
