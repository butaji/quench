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
        let slots = self.lane_global_slots(program, function, unit);
        reads.into_iter().chain(members).chain(slots).collect()
    }

    /// The PCs of `unit` whose `StoreName` (not a declaration's) or `LoadNameTypeof` reaches the
    /// global scope as `lane_global_reads` decides for a load, each with the global object's
    /// slot it holds (`Vm::stable_global_slot`), where it holds one.
    fn lane_global_slots(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> Vec<(usize, GlobalRead)> {
        let sites: Vec<(usize, crate::bytecode::Atom, bool)> = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)| {
            let store = match instruction.op() {
                crate::bytecode::Op::StoreName if instruction.boolean_field(crate::bytecode::InstructionField::B) == Some(false) => true,
                crate::bytecode::Op::LoadNameTypeof => false,
                _ => return None,
            };
            let function = match unit.origins[pc] {
                lane_unit::Origin::Caller(_) => function,
                lane_unit::Origin::Inlined { frame, .. } => unit.frames[frame].target.function,
            };
            let statically_global = program.function_get(function as usize).is_some_and(|body| body.names_resolve_statically(program))
                && self.programs.names_are_global(self.active_program);
            (statically_global && !self.atom_class(instruction.atom_index()).contains(super::super::object::AtomClass::INTERNAL))
                .then_some((pc, instruction.atom_index(), store))
        }).collect();
        sites.into_iter().filter_map(|(pc, atom, store)| Some((pc, GlobalRead::Slot(self.stable_global_slot(atom, store)?)))).collect()
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
