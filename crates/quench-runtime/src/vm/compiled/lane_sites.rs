//! What a lane unit's by-name loads read at compile time: the global scope's bindings.
use super::*;
use super::super::global_facts::{GlobalRead, GlobalReads, GlobalSlot, GlobalTable, StableCell, scalar_native};

impl<H: Host> Vm<H> {
    /// The PCs of `unit` whose by-name loads read the global scope as the lane's own does
    /// (`dispatch_fast::name_resolution`): in a function whose unresolved names are
    /// global and resolve statically, a name no internal binding owns; each with its constant
    /// where the name is an immutable global (`builtins::immutable_globals`), else with the
    /// cell it reads now as a Stable fact (`Vm::stable_global_read`).
    pub(in crate::vm) fn global_reads_in_lane(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit)
        -> crate::vm::global_facts::GlobalReads {
        // Before the loads hold their cells: a store reaches a cell no code holds, and the loads of
        // a name the unit stores read its slot (`stored`), typed by the slot's dependency, not folded
        // as a fact the store would retire each time.
        let slots = self.global_slots_in_lane(program, function, unit);
        // The first store of each name holds its slot.
        let mut stored: std::collections::HashMap<crate::bytecode::Atom, GlobalRead> = std::collections::HashMap::new();
        for (pc, _, slot) in slots.iter().filter(|(_, store, _)| *store) {
            stored.entry(unit.instructions[*pc].atom_index()).or_insert(*slot);
        }
        let immutable = super::super::builtins::immutable_globals();
        let reads: Vec<(usize, GlobalRead)> = self.global_name_loads(program, function, unit)
            .into_iter()
            .map(|(pc, atom, site)| {
                let name = self.atom_name(atom);
                let read = match immutable.iter().find(|(global, _)| *global == name) {
                    Some((_, value)) => GlobalRead::Constant(*value),
                    None => match stored.get(&atom) {
                        Some(slot) => *slot,
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
    /// (`global_reads_in_lane`), or the b-jit's, where a load of a global object's own data property that
    /// is no constant is held as a slot of the object's layout too, which its row reads live.
    pub(in crate::vm) fn global_table(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit, table: GlobalTable)
        -> GlobalReads {
        let reads = self.global_reads_in_lane(program, function, unit);
        let loads: Vec<(usize, GlobalRead)> = reads.range(0..=usize::MAX).map(|(pc, read)| (pc, *read)).collect();
        match table {
            GlobalTable::Lane => loads.into_iter().map(|(pc, read)| (pc, self.lane_native_read(unit, pc, read))).collect(),
            GlobalTable::Base => {
                let stores = self.lexical_stores(program, function, unit);
                let held = held_objects(&loads);
                loads.into_iter().map(|(pc, read)| {
                    let read = if held.contains(&pc) { read } else { self.slot_load(unit, pc, read) };
                    (pc, self.native_slot(pc, unit, read))
                }).chain(stores).collect()
            }
        }
    }

    /// The by-name load at `pc` of a global object's own data property that holds a scalar native (`parseInt`), as the
    /// lane reads it: a `NativeSlot` the call guards by identity (`lane_region::native_calls`), where the load is a
    /// `Stable` read of that same function; `read` otherwise.
    fn lane_native_read(&mut self, unit: &lane_unit::Unit, pc: usize, read: GlobalRead) -> GlobalRead {
        let GlobalRead::Stable { value, .. } = read else { return read };
        let instruction = unit.instructions[pc];
        if !matches!(crate::staged::Operation::<u64, ()>::decode_load_name(instruction, ()), Some(crate::staged::Operation::LoadName { .. })) {
            return read;
        }
        let atom = instruction.atom_index();
        let Some(slot) = self.stable_global_slot(atom, false) else { return read };
        match self.native_slot_of(slot, atom) {
            Some(native @ GlobalRead::NativeSlot { value: held, .. }) if held == value => native,
            _ => read,
        }
    }

    /// The by-name load at `pc` held as a slot of the global object (`GlobalRead::Slot`) that holds a scalar native
    /// (`native_slot_of`) becomes a `NativeSlot`, which the b-jit's call row and the lane's inline call guard by identity.
    fn native_slot(&mut self, pc: usize, unit: &lane_unit::Unit, read: GlobalRead) -> GlobalRead {
        let GlobalRead::Slot(slot) = read else { return read };
        let atom = unit.instructions[pc].atom_index();
        self.native_slot_of(slot, atom).unwrap_or(read)
    }

    /// The `NativeSlot` of the global object's own data property `atom` at `slot`, where it holds a scalar native now
    /// (`global_facts::scalar_native`): the native, its value, and the watched exact shape of the global object.
    fn native_slot_of(&mut self, slot: GlobalSlot, atom: crate::bytecode::Atom) -> Option<GlobalRead> {
        let Some(Some(value)) = self.own_plain_data(slot.global, atom) else { return None };
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(native), .. } = self.heap.function(value)? else { return None };
        let native = *native;
        if !scalar_native(native) { return None; }
        let holder = self.native_holder(slot.global)?;
        Some(GlobalRead::NativeSlot { slot, value, native, holder })
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
    /// (`dispatch_fast::name_resolution`): the function's unresolved names are global and resolve
    /// statically, and no internal binding owns the name.
    fn reads_global_scope(&self, program: &ResidualProgram, function: u32, atom: crate::bytecode::Atom) -> bool {
        program.function_get(function as usize).is_some_and(|body| body.names_resolve_statically(program))
            && self.programs.names_are_global(self.active_program)
            && (!self.atom_class(atom).contains(super::super::object::AtomClass::INTERNAL)
                || crate::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&self.atom_name(atom)))
    }

    /// What each call site's callee register holds, by PC: the Stable global cell its by-name read took, where that read
    /// is the register's only definition reaching the call and the cell holds the function the site recorded (`recorded`,
    /// by PC); or the body's own depth-0 capture of a declaration nothing writes, which the body's entry checks against
    /// the recorded function (`Held::Captured`). Code that folds the callee holds the fact and tests nothing at the call.
    pub(in crate::vm) fn stable_callees(&mut self, program: &ResidualProgram, function: u32, code: &[crate::bytecode::WideInstruction], recorded: &[Option<crate::value::Value>])
        -> Vec<Option<super::base_region::callees::Held>> {
        use super::base_region::callees::Held;
        let entered = branch_targets(code);
        recorded.iter().enumerate().map(|(call, callee)| {
            let callee = (*callee)?;
            let load = code[straight_definition(code, &entered, call)?];
            if load.op() == crate::bytecode::Op::LoadCapture && load.result_flags() == 0 && load.capture_depth() == 0 {
                let declared = program.function_get(function as usize)?.declared_captures.contains(&load.capture_slot());
                return declared.then_some(Held::Captured { slot: load.capture_slot(), callee: callee.to_bits() });
            }
            if !load.op().is_throwing_name_load() { return None; }
            let atom = load.atom_index();
            if !self.reads_global_scope(program, function, atom) {
                return None;
            }
            match self.stable_global_read(program, atom, load.cache_site_index()) {
                GlobalRead::Stable { value, fact, .. } if value == callee => Some(Held::Global(fact)),
                _ => None,
            }
        }).collect()
    }

    /// The native function each call site's callee register holds, by PC, where a field read defines it: a method
    /// of strings or arrays (`method_native`), or a member of a Stable global read by name (`Math.floor`). Each
    /// with its identity, which the call guards, and the holder whose values the code depends on (the b-jit's
    /// builtin rows, `base_region::call_plan::builtin`).
    pub(in crate::vm) fn native_callees(&mut self, program: &ResidualProgram, function: u32, code: &[crate::bytecode::WideInstruction])
        -> Vec<Option<(crate::value::Value, crate::heap::Native, u32)>> {
        let entered = branch_targets(code);
        (0..code.len()).map(|call| {
            if !code[call].op().is_call() { return None; }
            let member = straight_definition(code, &entered, call)?;
            self.native_member_callee(program, function, code, &entered, member)
        }).collect()
    }

    /// The native a field read at `member` defines, as `native_callees` names it.
    fn native_member_callee(&mut self, program: &ResidualProgram, function: u32, code: &[crate::bytecode::WideInstruction],
        entered: &cranelift_codegen::bitset::CompoundBitSet, member: usize) -> Option<(crate::value::Value, crate::heap::Native, u32)> {
        if let Some(GlobalRead::NativeMember { value, native, holder, .. }) = self.method_native(&code[member]) {
            return Some((value, native, holder));
        }
        let crate::bytecode::FieldLookup::Atom { atom, base, .. } = code[member].field_lookup()? else { return None };
        let read = member.checked_sub(1)?;
        if entered.contains(member) || base.register_index()? != code[read].result_register() { return None; }
        let load = code[read];
        if !load.op().is_throwing_name_load() || !self.reads_global_scope(program, function, load.atom_index()) { return None; }
        let GlobalRead::Stable { value: object, .. } = self.stable_global_read(program, load.atom_index(), load.cache_site_index()) else { return None };
        let value = self.own_plain_data(object, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(native), .. } = self.heap.function(value)? else { return None };
        let native = *native;
        Some((value, native, self.native_holder(object)?))
    }

    /// The field read `instruction` of a method of strings, arrays or DataViews that a call runs inline
    /// (`lane_region::native_calls`), where the realm's prototype holds that native as an own
    /// data property now. The receiver is not known: the call guards the callee it reads.
    pub(in crate::vm) fn method_native(&mut self, instruction: &crate::bytecode::WideInstruction) -> Option<GlobalRead> {
        if !instruction.op().is_get_field() { return None; }
        let Some(crate::bytecode::FieldLookup::Atom { atom, .. }) = instruction.field_lookup() else { return None };
        let name = self.atom_name(atom);
        let row = crate::vm::native_rows::lane_method(name)?;
        let native = row.native;
        let holder = match row.holder {
            crate::vm::native_rows::Holder::ArrayPrototype => self.array_proto,
            crate::vm::native_rows::Holder::DataViewPrototype => self.data_view_proto,
            _ => self.string_proto,
        };
        let value = self.own_plain_data(holder, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(held), .. } = self.heap.function(value)? else { return None };
        (*held == native).then(|| self.native_holder(holder)).flatten().map(|holder| GlobalRead::NativeMember { value, native, holder, folds: false, cell: None })
    }

    /// The PCs of `unit` whose `StoreName` (not a declaration's) or `LoadNameTypeof` reaches the
    /// global scope as `global_reads_in_lane` decides for a load, each with the global object's
    /// slot it holds (`Vm::stable_global_slot`), where it holds one.
    fn global_slots_in_lane(&mut self, program: &ResidualProgram, function: u32, unit: &lane_unit::Unit) -> Vec<(usize, bool, GlobalRead)> {
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
        let cell = match *read {
            GlobalRead::Stable { fact, global, realm_offset, .. } => Some(StableCell { fact, global, realm_offset }),
            _ => None,
        };
        let (base, member) = (unit.instructions.get(pc)?.result_register(), unit.instructions.get(pc + 1)?);
        if !matches!(crate::staged::Operation::<u64, ()>::of(*member), Some(crate::staged::Operation::GetField { .. })) { return None; }
        let Some(crate::bytecode::FieldLookup::Atom { base: read_base, atom, .. }) = member.field_lookup() else { return None };
        if read_base.register_index() != Some(base) { return None; }
        let value = self.own_plain_data(object, atom)??;
        let crate::heap::FunctionRecord { kind: crate::vm::FunctionKind::Native(native), .. } = self.heap.function(value)? else { return None };
        let native = *native;
        Some((pc + 1, GlobalRead::NativeMember { value, native, holder: self.native_holder(object)?, folds: true, cell }))
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

/// The by-name loads of a global object a folded member read holds (`GlobalRead::NativeMember`'s `cell`): a Stable read at `pc` whose
/// member read at `pc + 1` holds the same cell fact. The base table keeps these as held rows (`globals::global_row`), with no live slot
/// read per use; the body subscribes to the fact, so a reassignment retires it.
fn held_objects(loads: &[(usize, GlobalRead)]) -> Vec<usize> {
    loads.iter().filter_map(|(pc, read)| {
        let GlobalRead::Stable { fact, .. } = read else { return None };
        let member = |next: &(usize, GlobalRead)| next.0 == pc + 1 && matches!(next.1, GlobalRead::NativeMember { cell: Some(cell), .. } if cell.fact == *fact);
        loads.iter().any(member).then_some(*pc)
    }).collect()
}
