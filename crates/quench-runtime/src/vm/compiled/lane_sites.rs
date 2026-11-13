//! What a lane unit's by-name loads read at compile time: the global scope's bindings.
use super::*;
use super::super::global_facts::{GlobalRead, GlobalReads, GlobalTable};

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::lane_load_name_first`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns; each with its constant
    /// where the name is an immutable global (`builtins::immutable_globals`), else with the
    /// cell it reads now as a Stable fact (`Vm::stable_global_read`).
    pub(in crate::vm) fn lane_global_reads(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit)
        -> crate::vm::global_facts::GlobalReads {
        let reads: Vec<(usize, GlobalRead)> = unit.instructions.iter().enumerate()
            .filter(|(_, instruction)| instruction.op().is_throwing_name_load())
            .filter(|(pc, instruction)| {
                let function = match unit.origins[*pc] {
                    lane_unit::Origin::Caller(_) => function,
                    lane_unit::Origin::Inlined { frame, .. } => unit.frames[frame].target.function,
                };
                self.reads_global_scope(program, function, instruction.atom_index())
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

    /// The by-name global accesses of `unit` as `table`'s tier takes them: the lane's own
    /// (`lane_global_reads`), or the b-jit's, where a load of a global object's own data property that
    /// is no constant is held as a slot of the object's layout too, which its row reads live.
    pub(in crate::vm) fn global_table(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit, table: GlobalTable)
        -> GlobalReads {
        let reads = self.lane_global_reads(program, function, unit);
        match table {
            GlobalTable::Lane => reads,
            GlobalTable::Base => {
                let loads: Vec<(usize, GlobalRead)> = reads.range(0..=usize::MAX).map(|(pc, read)| (pc, *read)).collect();
                loads.into_iter().map(|(pc, read)| (pc, self.slot_load(unit, pc, read))).collect()
            }
        }
    }

    /// The by-name load at `pc` held as a slot of the global object's layout where its read is no
    /// constant and the object owns the name as a data property (`Vm::stable_global_slot`); `read` otherwise.
    fn slot_load(&mut self, unit: &lane_unit::Unit, pc: usize, read: GlobalRead) -> GlobalRead {
        let instruction = unit.instructions[pc];
        let loads = matches!(read, GlobalRead::Stable { .. } | GlobalRead::Dynamic) && instruction.op().is_throwing_name_load();
        loads.then(|| self.stable_global_slot(instruction.atom_index(), false)).flatten().map_or(read, GlobalRead::Slot)
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

    /// Whether a by-name load of `atom` in `function` reads the global scope as the lane's own load does
    /// (`dispatch_fast::lane_load_name_first`): the function's unresolved names are global and resolve
    /// statically, and no internal binding owns the name.
    fn reads_global_scope(&self, program: &ResidualProgram, function: u32, atom: crate::bytecode::Atom) -> bool {
        program.function_get(function as usize).is_some_and(|body| body.names_resolve_statically(program))
            && self.programs.names_are_global(self.active_program)
            && (!self.atom_class(atom).contains(super::super::object::AtomClass::INTERNAL)
                || crate::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&self.atom_name(atom)))
    }

    /// The Stable fact each call site's callee register holds, by PC: the global cell its by-name
    /// read took, where that read is the register's only definition reaching the call and the cell
    /// holds the function the site recorded (`recorded`, by PC). Code that folds the callee holds
    /// the fact as a dependency (`StableFacts::globals`) and tests nothing at the call.
    pub(in crate::vm) fn stable_callees(&mut self, program: &ResidualProgram, function: u32, code: &[crate::bytecode::WideInstruction], recorded: &[Option<crate::value::Value>])
        -> Vec<Option<crate::heap::GlobalFact>> {
        let entered = branch_targets(code);
        recorded.iter().enumerate().map(|(call, callee)| {
            let callee = (*callee)?;
            let load = code[straight_definition(code, &entered, call)?];
            let atom = load.atom_index();
            if !load.op().is_throwing_name_load() || !self.reads_global_scope(program, function, atom) {
                return None;
            }
            match self.stable_global_read(program, atom, load.cache_site_index()) {
                GlobalRead::Stable { value, fact, .. } if value == callee => Some(fact),
                _ => None,
            }
        }).collect()
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

/// The PCs a jump or a branch table enters: where control may arrive other than by falling through.
fn branch_targets(code: &[crate::bytecode::WideInstruction]) -> std::collections::BTreeSet<usize> {
    let mut entered = std::collections::BTreeSet::new();
    for (pc, instruction) in code.iter().enumerate() {
        crate::bytecode::for_each_instruction_successor(pc, *instruction, |successor| {
            if successor != pc + 1 {
                entered.insert(successor);
            }
        });
    }
    entered
}

/// The PC of the instruction defining the callee register of the call at `call`, if it is the only
/// one reaching it: the nearest before the call that writes the register, with no jump landing
/// between the two.
fn straight_definition(code: &[crate::bytecode::WideInstruction], entered: &std::collections::BTreeSet<usize>, call: usize) -> Option<usize> {
    let callee = code[call].register_b();
    let defined = (0..call).rev().find(|pc| code[*pc].written_registers().any(|register| register == callee))?;
    entered.range(defined + 1..=call).next().is_none().then_some(defined)
}
