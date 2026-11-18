//! What a lane unit's by-name loads read at compile time: the global scope's bindings.
use super::*;
use super::super::global_facts::{GlobalRead, GlobalReads, GlobalTable};

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::lane_name_resolution`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns; each with its constant
    /// where the name is an immutable global (`builtins::immutable_globals`), else with the
    /// cell it reads now as a Stable fact (`Vm::stable_global_read`).
    pub(in crate::vm) fn lane_global_reads(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit)
        -> crate::vm::global_facts::GlobalReads {
        // Before the loads hold their cells: a store reaches a cell no code holds, and the loads of
        // a name the unit stores read its slot (`stored`), typed by the slot's dependency, not folded
        // as a fact the store would retire each time.
        let slots = self.lane_global_slots(program, function, unit);
        let stored: Vec<(crate::bytecode::Atom, GlobalRead)> = slots.iter().filter(|(_, store, _)| *store)
            .map(|(pc, _, slot)| (unit.instructions[*pc].atom_index(), *slot)).collect();
        let reads: Vec<(usize, GlobalRead)> = self.global_name_loads(program, function, unit)
            .into_iter()
            .map(|(pc, atom, site)| {
                let name = self.atom_name(atom);
                let immutable = super::super::builtins::immutable_globals().into_iter().find(|(global, _)| *global == name);
                let read = match immutable {
                    Some((_, value)) => GlobalRead::Constant(value),
                    None => match stored.iter().find(|(stored, _)| *stored == atom) {
                        Some((_, slot)) => *slot,
                        None => self.stable_global_read(program, atom, site),
                    },
                };
                (pc, read)
            })
            .collect();
        let members: Vec<(usize, GlobalRead)> = reads.iter().filter_map(|(pc, read)| self.native_member(unit, *pc, read)).collect();
        let methods: Vec<(usize, GlobalRead)> = unit.instructions.iter().enumerate()
            .filter_map(|(pc, instruction)| Some((pc, self.method_native(instruction)?))).collect();
        reads.into_iter().chain(members).chain(methods).chain(slots.into_iter().map(|(pc, _, read)| (pc, read))).collect()
    }

    /// The loads of `unit` that read a global by name, as (PC, atom, cache site): those whose name
    /// resolves in the global scope of the function that holds them.
    fn global_name_loads(&self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> Vec<(usize, crate::bytecode::Atom, u16)> {
        unit.instructions.iter().enumerate()
            .filter(|(_, instruction)| matches!(crate::staged::Operation::<u64, ()>::decode_load_name(**instruction, ()), Some(crate::staged::Operation::LoadName { .. })))
            .filter(|(pc, instruction)| {
                let function = match unit.origins[*pc] {
                    lane_unit::Origin::Caller(_) => function,
                    lane_unit::Origin::Inlined { frame, .. } => unit.frames[frame].target.function,
                };
                self.reads_global_scope(program, function, instruction.atom_index())
            })
            .map(|(pc, instruction)| (pc, instruction.atom_index(), instruction.cache_site_index()))
            .collect()
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
                let stores = self.lexical_stores(program, function, unit);
                loads.into_iter().map(|(pc, read)| (pc, self.slot_load(unit, pc, read))).chain(stores).collect()
            }
        }
    }

    /// The by-name load at `pc` held as a slot of the global object's layout where its read is no
    /// constant and the object owns the name as a data property (`Vm::stable_global_slot`), or as a global lexical
    /// binding's word read live (`Vm::global_lexical_slot`); `read` otherwise.
    fn slot_load(&mut self, unit: &lane_unit::Unit, pc: usize, read: GlobalRead) -> GlobalRead {
        let instruction = unit.instructions[pc];
        let loads = matches!(read, GlobalRead::Stable { .. } | GlobalRead::Dynamic) && matches!(crate::staged::Operation::<u64, ()>::decode_load_name(instruction, ()), Some(crate::staged::Operation::LoadName { .. }));
        if !loads {
            return read;
        }
        let atom = instruction.atom_index();
        match self.stable_global_slot(atom, false) {
            Some(slot) => GlobalRead::Slot(slot),
            None => self.global_lexical_slot(atom, false).map_or(read, GlobalRead::Lexical),
        }
    }

    /// The `StoreName`s of `unit` (not a declaration's) of a global lexical binding that is assignable and that no code holds the value of
    /// (`Vm::global_lexical_slot`), each with the binding's word, which its row writes live; a binding not `Mutable` yet is held
    /// (`GlobalRead::Held`) until its writes make it so.
    fn lexical_stores(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> Vec<(usize, GlobalRead)> {
        let sites = self.global_store_sites(program, function, unit);
        sites.into_iter().filter(|(_, _, store)| *store).filter_map(|(pc, atom, _)| {
            let live = self.global_lexical_slot(atom, true).map(GlobalRead::Lexical);
            Some((pc, live.or_else(|| self.held_lexical(atom).map(GlobalRead::Held))?))
        }).collect()
    }

    /// Whether a by-name load of `atom` in `function` reads the global scope as the lane's own load does
    /// (`dispatch_fast::lane_name_resolution`): the function's unresolved names are global and resolve
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
            if !matches!(crate::staged::Operation::<u64, ()>::decode_load_name(load, ()), Some(crate::staged::Operation::LoadName { .. })) { return None; }
            let atom = load.atom_index();
            if !self.reads_global_scope(program, function, atom) {
                return None;
            }
            match self.stable_global_read(program, atom, load.cache_site_index()) {
                GlobalRead::Stable { value, fact, .. } if value == callee => Some(fact),
                _ => None,
            }
        }).collect()
    }

    /// The field read `instruction` of a method of strings or arrays that a call runs inline
    /// (`lane_region::native_calls`), where the realm's prototype holds that native as an own
    /// data property now. The receiver is not known: the call guards the callee it reads.
    fn method_native(&mut self, instruction: &crate::bytecode::WideInstruction) -> Option<GlobalRead> {
        /// The methods a call runs inline, by name.
        const METHODS: [(&str, crate::vm::Native); 4] = [("charCodeAt", crate::vm::Native::StringCharCodeAt), ("charAt", crate::vm::Native::StringCharAt),
            ("push", crate::vm::Native::ArrayPush), ("pop", crate::vm::Native::ArrayPop)];
        if !matches!(crate::staged::Operation::<u64, ()>::of(*instruction), Some(crate::staged::Operation::GetField { .. })) { return None; }
        let Some(crate::bytecode::FieldLookup::Atom { atom, .. }) = instruction.field_lookup() else { return None };
        let name = self.atom_name(atom);
        let (_, native) = METHODS.iter().find(|(method, _)| *method == name)?;
        let holder = if matches!(native, crate::vm::Native::ArrayPush | crate::vm::Native::ArrayPop) { self.array_proto } else { self.string_proto };
        let value = self.own_plain_data(holder, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(held), .. } = self.heap.function(value)? else { return None };
        let native = *native;
        (*held == native).then(|| self.native_holder(holder)).flatten().map(|holder| GlobalRead::NativeMember { value, native, holder })
    }

    /// The PCs of `unit` whose `StoreName` (not a declaration's) or `LoadNameTypeof` reaches the
    /// global scope as `lane_global_reads` decides for a load, each with the global object's
    /// slot it holds (`Vm::stable_global_slot`), where it holds one.
    fn lane_global_slots(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> Vec<(usize, bool, GlobalRead)> {
        let sites = self.global_store_sites(program, function, unit);
        sites.into_iter().filter_map(|(pc, atom, store)| Some((pc, store, GlobalRead::Slot(self.stable_global_slot(atom, store)?)))).collect()
    }

    /// The PCs of `unit` with an access (the atom's, and whether it is a store) the global scope answers as the lane's own does: a
    /// `StoreName` that is no declaration's or a `LoadNameTypeof` of a name no internal binding owns, in a function whose names are global.
    fn global_store_sites(&self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> Vec<(usize, crate::bytecode::Atom, bool)> {
        unit.instructions.iter().enumerate().filter_map(|(pc, instruction)| {
            let store = match crate::staged::Operation::<(), ()>::decode(*instruction, crate::staged::DecodeContext::EMPTY)? {
                crate::staged::Operation::GlobalWrite { initialize: false, .. } => true,
                crate::staged::Operation::GlobalTypeof { .. } => false,
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
        }).collect()
    }

    /// The field read that follows the global read `read` at `pc` and reads its register, where
    /// the member it names is an own data property holding a native function now.
    fn native_member(&mut self, unit: &lane_unit::Unit, pc: usize, read: &GlobalRead) -> Option<(usize, GlobalRead)> {
        let (GlobalRead::Constant(object) | GlobalRead::Stable { value: object, .. }) = *read else { return None };
        let (base, member) = (unit.instructions.get(pc)?.result_register(), unit.instructions.get(pc + 1)?);
        if !matches!(crate::staged::Operation::<u64, ()>::of(*member), Some(crate::staged::Operation::GetField { .. })) { return None; }
        let Some(crate::bytecode::FieldLookup::Atom { base: read_base, atom, .. }) = member.field_lookup() else { return None };
        if read_base.register_index() != Some(base) { return None; }
        let value = self.own_plain_data(object, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(native), .. } = self.heap.function(value)? else { return None };
        let native = *native;
        Some((pc + 1, GlobalRead::NativeMember { value, native, holder: self.native_holder(object)? }))
    }

    /// The exact shape of `holder`, an object whose own native functions code calls inline,
    /// made a watched prototype so that a write to one of its values retires that code.
    fn native_holder(&mut self, holder: Value) -> Option<u32> {
        self.heap.mark_prototype(holder);
        Some(self.object_data(holder)?.shape())
    }
}

/// The PCs a jump or a branch table enters: where control may arrive other than by falling through.
fn branch_targets(code: &[crate::bytecode::WideInstruction]) -> cranelift_codegen::bitset::CompoundBitSet {
    let mut entered = cranelift_codegen::bitset::CompoundBitSet::with_capacity(code.len());
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
fn straight_definition(code: &[crate::bytecode::WideInstruction], entered: &cranelift_codegen::bitset::CompoundBitSet, call: usize) -> Option<usize> {
    let callee = code[call].register_b();
    let defined = (0..call).rev().find(|pc| code[*pc].written_registers().any(|register| register == callee))?;
    (defined + 1..=call).all(|pc| !entered.contains(pc)).then_some(defined)
}
