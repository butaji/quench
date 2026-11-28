use super::*;

/// Lowers `unit` (`lane_unit::unit`) as a lane body, whose field cache sites `fields`
/// gives the proved shapes and slots of; `None` where it does not lower.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_body(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &DirectSites,
    frame_captures: &[(usize, usize)], entry: Option<usize>,
    kinds: &[super::super::body_views::ObservedKind], views: ElementViews,
    admitted: super::super::admission::CompiledUnit,
) -> Option<LoweredLane> {
    let isa = super::super::jit_backend::target_isa()?;
    DECLINED.with(|declined| declined.set(None));
    UNLOWERED.with(|unlowered| unlowered.borrow_mut().clear());
    MISMATCH.with(|mismatch| mismatch.set(false));
    let body = RegionSpecializer { function_id, unit, helpers, fields, global_reads, facts: vm,
        generic, direct, frame_captures, isa: &*isa, kinds, admitted,
        plan: RegionPlan { coverage: entry.map_or(Coverage::Prefix, Coverage::PrefixFrom),
            completion: Completion::Body, views } };
    // The scalar entry lowers first: the body calls it only where it exists.
    let abi = scalar::ScalarAbi::of(program, function_id, unit, direct, vm.facts).filter(|_| entry.is_none());
    let scalar = abi.and_then(|abi| scalar::lowered(program, RegionSpecializer { generic: &[], facts: VmFacts { scalar: Some(abi), ..vm }, plan: scalar::plan(abi), ..body }));
    let region = RegionSpecializer { facts: VmFacts { scalar: abi.filter(|_| scalar.is_some()), ..vm }, ..body }.specialize(program)
        .inspect_err(|_| if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            let (why, pc) = DECLINED.with(|declined| declined.get()).unwrap_or(("a decline no note names", usize::MAX));
            let op = unit.instructions.get(pc).map(|instruction| format!("{:?}", instruction.op())).unwrap_or_default();
            let unlowered = UNLOWERED.with(|unlowered| unlowered.borrow().iter().cloned().collect::<Vec<_>>().join(","));
            eprintln!("aot-lanes: function {function_id}: lowering declines: {why} {op} [leaves at {unlowered}]");
        }).ok()?;
    // The proven entry lowers the same body with its parameters' entry guards taken as proved (`VmFacts::proven_entry`).
    // It is kept only where it folds exactly the body's Stable facts, which its publication takes once for both.
    let params = program.function_get(function_id as usize).is_some_and(|function| function.params > 0);
    let proven = (entry.is_none() && params).then(|| RegionSpecializer { facts: VmFacts { proven_entry: true, ..vm }, ..body }
        .specialize(program).ok()).flatten()
        .filter(|proven| proven.stable == region.stable && proven.fault_poll == region.fault_poll && proven.hoists_arrays == region.hoists_arrays
            && proven.entry_types.iter().any(|ty| *ty != Type::ANY))
        .map(|proven| (proven.function, proven.entry_types));
    super::super::residuals::report_clif(function_id, &region.function);
    let clif = native_dump_directory().map(|_| region.function.to_string());
    Some(LoweredLane { region, clif, scalar, proven })
}

/// What an ahead lane body reads of its image. Its leaves return their exit words, as a JIT lane's
/// do: a base body is entered only at its start (a record inside it has no resume stub to load
/// its pinned registers), and a base body leaves the same way.
#[derive(Clone, Copy)]
pub(crate) struct AheadLeaves<'a> {
    /// By unit PC, the calls the closed world proves whose records the body reads (`ProvenCall`): one, or one per
    /// function of a method read's closed set (`AotFacts::call_targets`).
    pub(crate) calls: &'a BTreeMap<usize, Vec<ProvenCall>>,
    /// By constant pool slot, the heap value key (`HeapValueRecipe::String`) of each string
    /// constant the unit loads.
    pub(crate) strings: &'a BTreeMap<usize, u32>,
}

/// The position a failed entry contract takes in a lane body's leave set
/// (`LaneExit::Entry`): past every instruction, so it names none of them. A plan with it
/// proves no input contract.
pub(crate) fn entry_leave(unit: &super::super::lane_unit::Unit) -> usize {
    unit.instructions.len()
}

/// A loop's entry types the function's bytecode proves: a register the loop reads but never writes,
/// whose every definition before or after the loop loads a constant that is no Number (a string,
/// a boolean, `null`), has the join of those constants' types, which the entry guards once. A
/// relation reading it is then decided by the interpreter's rule (`Lowering::decided_relation`)
/// instead of being taken for a Number comparison whose entry guard always fails.
pub(super) fn seed_constant_entries(instructions: &[WideInstruction], program: &ResidualProgram,
    (header, backedge): (usize, usize), initial: &mut [Type]) {
    let inside = header..=backedge;
    let written: std::collections::BTreeSet<Register> = instructions.get(inside.clone()).into_iter().flatten()
        .filter_map(|instruction| instruction.defined_register()).collect();
    let mut seeded = std::collections::BTreeMap::<Register, Option<Type>>::new();
    for (_, instruction) in instructions.iter().enumerate().filter(|(pc, _)| !inside.contains(pc)) {
        let Some(register) = instruction.defined_register() else { continue };
        if written.contains(&register) { continue; }
        // A definition by a constant-table index (the staged row of a heap constant, a string, has no
        // stage word to decode, so the instruction's layout names it); the table types it.
        let ty = (instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::ConstantIndex)
            .then(|| program.constants.get(instruction.constant_index())).flatten()
            .map(super::super::facts::constant_type).filter(|ty| super::super::facts::numberless(*ty));
        let slot = seeded.entry(register).or_insert(ty);
        *slot = slot.zip(ty).map(|(left, right)| left.join(right));
    }
    for (register, ty) in seeded {
        if let (Some(ty), Some(slot)) = (ty, initial.get_mut(usize::from(register))) && *slot == Type::ANY { *slot = ty; }
    }
}
