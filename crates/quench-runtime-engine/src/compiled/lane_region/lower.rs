use super::*;

/// Build the typed residual and validate every value/CFG edge before publication.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &DirectSites,
    frame_captures: &[(usize, usize)],
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    kinds: &[super::super::body_views::ObservedKind],
    coverage: Coverage,
    completion: Completion,
    views: ElementViews,
) -> Option<LoweredRegion> {
    // A loop entry may take the rotated residual of its proven condition.
    let residual = super::super::static_loops::specialize(program, unit, coverage.entry())
        .map(|instructions| super::super::lane_unit::Unit { instructions, origins: unit.origins.clone(),
            frames: unit.frames.clone(), markers: unit.markers.clone(), caller_registers: unit.caller_registers,
            registers: unit.registers, end: unit.end });
    let unit = residual.as_ref().unwrap_or(unit);
    let naming = super::super::NamingSymbols::enter(helpers);
    crate::dispatch_fast::lane_body_stats::lowering_function(function_id);
    let function = program.function_get(function_id as usize)?;
    let numeric = completion.numeric();
    if numeric && (!crate::js_compiled::function_candidate(function, program)
        || !scalar::numeric_unit(program, unit, completion != Completion::Number, direct) || function.params > unit.caller_registers) { return None; }
    if fields.iter().flatten().flatten().any(|field| !field.valid()) { declined("an invalid field site", usize::MAX); return None; }
    if unit.instructions.is_empty() || unit.instructions.len() > MAX_NATIVE_RECORDS {
        declined("past the native record count", usize::MAX);
        return None;
    }
    let signature = completion.signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::user(LANE_REGION_NAMESPACE, function_id), signature);
    // Every access this lowering emits is tagged with its alias region (`regions::flags`), so the
    // function names them before anything verifies or compiles it (declaring again is a no-op).
    super::super::regions::declare_regions(&mut native);
    // The SSA builder's tables come back from the last lowering that finished, which
    // `finalize` left empty; one that gave up drops its context with it.
    let mut builder_context = super::super::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let parameters = builder.block_params(entry_block).to_vec();
    let [context, entry_this, entry_captures, registers_base] = scalar::entry_words(&mut builder, completion, &parameters)?;
    let mut stage = emit(&mut builder);
    let payload = if numeric { stage.raw_const(Lane::I64, 0) } else {
        stage.raw_load(Lane::I64, regions::dispatch(), context, std::mem::offset_of!(CallContext, head.adapter_payload) as i32) };
    // An image body is linked away from its compile, so a fault poll would name the compiling
    // process's fault address: its back edge polls `context.interrupt` instead (no trapping check).
    let fault_poll = !matches!(helpers, LaneHelpers::Ahead)
        && match coverage { Coverage::Loop { poll, .. } => poll, _ => true }
        && super::super::FAULT_POLL_SUPPORTED && quench_runtime::bytecode::back_edges_of(unit.instructions.iter().copied()).next().is_some();
    let mut seen_headers: BitSet = BitSet::with_capacity(unit.instructions.len());
    let mut shared_headers = BitSet::with_capacity(unit.instructions.len());
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Jump)) { continue; }
        let target = instruction.jump_target() as usize;
        if target > pc { continue; }
        if seen_headers.contains(target) { shared_headers.insert(target); }
        seen_headers.insert(target);
    }
    let interrupt = super::super::native_interrupt_pointer(&mut builder, context, fault_poll);
    let helpers = if let LaneHelpers::Linked(symbol, narrow_table) = helpers {
        let signature = builder.import_signature(native_lane_helpers_signature(isa));
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            super::super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol));
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        LaneHelpers::Bound(emit(&mut builder).call_function(target, &[])[0], narrow_table)
    } else { helpers };
    let interrupt_owner = fault_poll.then(|| polls::interrupt_owner_call(&mut builder, helpers, context, isa));
    let count = usize::from(unit.registers);
    let live = if unit.registers > quench_runtime::compile::liveness::ROOT_MAP_REGISTERS {
        Liveness::Frames {
            caller: program.register_root_masks(function_id),
            frames: unit.frames.iter().map(|frame| program.register_root_masks(frame.target.function)).collect(),
        }
    } else {
        Liveness::Unit(unit.register_masks(program, function_id))
    };
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: vec![None; count],
        long: vec![None; count],
        float: vec![None; count],
    };
    // An alternative of a call's switch starts a block: a callee other than the one before
    // it branches there (`Lowering::inline_call`).
    let mut leaders = leaders(&unit.instructions);
    let entry_pc = coverage.entry();
    if !matches!(unit.origins.get(entry_pc), Some(super::super::lane_unit::Origin::Caller(_))) { declined("the entry is not the caller's", entry_pc); return None; }
    leaders.insert(entry_pc);
    for alternative in unit.frames.iter().filter_map(|frame| frame.alternative) {
        if alternative < unit.instructions.len() { leaders.insert(alternative); }
    }
    // A handler's block is entered by the throws it catches, edges no jump names.
    let catches = catches::catch_edges(program, function, unit, coverage, completion);
    for catch in catches.iter().flatten() { leaders.insert(catch.target); }
    let blocks: Vec<_> = (0..unit.instructions.len()).map(|pc| leaders.contains(pc).then(|| builder.create_block())).collect();
    let (leave, stop) = (builder.create_block(), builder.create_block());
    builder.append_block_param(leave, types::I32);
    builder.append_block_param(leave, types::I64);
    builder.append_block_param(stop, types::I64);
    builder.set_cold_block(leave);
    builder.set_cold_block(stop);
    // The typed array kind each access of the unit's own function proves (`FunctionFacts::element_view`).
    let element_facts = vm.facts;
    let element_kind = |pc: usize| match unit.origins.get(pc) {
        Some(super::super::lane_unit::Origin::Caller(pc)) => element_facts?.function_facts(function_id).element_view(*pc),
        _ => None,
    };
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        params: usize::from(function.params),
        function_id,
        helpers,
        fields,
        kinds,
        interrupt,
        fault_poll,
        trips: None,
        reentries: smallvec::SmallVec::new(),
        polls: smallvec::SmallVec::new(),
        poll_latches: super::super::SecondaryMap::new(),
        shared_headers,
        global_reads,
        plans: bindings::CallPlans::new(program, vm.facts, vm.graph),
        vm,
        stable: Default::default(),
        generic,
        proving_entry: false, left_here: false, held_view: None, hole_leaves: false, entry_exit: None,
        direct,
        frame_captures,
        context,
        payload,
        registers_base,
        entry_words: (entry_this, entry_captures),
        callee_slots: Vec::new(),
        declared_definitions: Vec::new(),
        declared_this: None,
        declared_rest: None,
        declared_members: Vec::new(),
        declared_member_start: None,
        registers,
        written: BitSet::with_capacity(count),
        consumed: BitSet::with_capacity(unit.instructions.len()),
        blocks,
        exits: std::iter::repeat_with(Vec::new).take(unit.instructions.len() + 1).collect(),
        shared_exits: Vec::new(),
        demands: demand::analyze(program, unit, &leaders, &live),
        entry: None,
        native: 0,
        backedge: false,
        pending: Vec::new(),
        call_out_stop: None,
        // A static typed region's calls are guarded leaves (`guarded_call_instruction`); here any call-like one is native.
        native_calls: completion.status_and_pc()
            && unit.instructions.iter().any(|instruction| instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Call),
        call_budget: None,
        facts: super::super::facts::Facts::default(),
        live,
        valid: true,
        coverage,
        completion,
        number_returns: smallvec::SmallVec::new(),
        hoists: BTreeMap::new(),
        unit_leaders: std::cell::OnceCell::new(),
        intrinsics: Vec::new(),
        hoisted: BTreeMap::new(),
        scalar_values: BTreeMap::new(),
        prototype_clear: BTreeMap::new(),
        int_only: BTreeSet::new(),
        construct_ready: Vec::new(),
        loop_arrays: Vec::new(), loop_strings: Vec::new(), string_vars: BTreeMap::new(), pending_unit: None, loop_shapes: Vec::new(), arrays_fixed: false,
        array_vars: BTreeMap::new(),
        views,
        loop_fields: Vec::new(),
        loop_stores: BTreeMap::new(),
        ranges: int_ranges::IntRanges::new(0),
        at: 0,
        field_regions: BTreeMap::new(),
        forward: Vec::new(), sealable: BitSet::new(),
        arrived: BTreeMap::new(), joined_virtuals: BTreeMap::new(), loop_carry: BTreeMap::new(), carried_typed: BTreeMap::new(),
        entered: BitSet::with_capacity(unit.instructions.len()),
        finishes: false,
        leave,
        stop,
        raises: catches::raises(&catches),
        catches,
    };
    lowering.field_regions = lowering.declare_fields();
    let instructions = unit.instructions.clone();
    for instruction in &instructions {
        for register in writes(*instruction).into_iter().map(usize::from).filter(|r| *r < count) {
            lowering.written.insert(register);
        }
    }
    let mut initial = completion.initial_types(count, usize::from(function.params));
    if let Coverage::Loop { header, backedge, .. } = coverage {
        seed_constant_entries(&instructions, program, (header, backedge), &mut initial);
    }
    if !numeric {
        if let Some(caller) = match unit.origins.get(entry_pc) { Some(super::super::lane_unit::Origin::Caller(pc)) => Some(*pc), _ => None } {
            int_inputs::solved_entry_types(vm.facts, function_id, function, caller, &lowering.written, &mut initial);
        }
        lowering.declared_definitions = declared::declared_local_types(program, function);
        int_inputs::closed_local_types(vm, function_id, function, &mut lowering.declared_definitions);
        lowering.declared_this = declared::declared_this(program, function);
        lowering.declared_rest = declared::declared_rest_read(program, function, &lowering.written);
        lowering.declared_members = declared::declared_member_reads(program, function, &lowering.written);
        lowering.declared_member_start = function.source_span.map(|span| span.start);
    }
    let own_shape = |pc: usize| {
        let instruction = instructions[pc];
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) || instruction.result_flags() != 0 { return None; }
        // A native method's read names its holder's function whatever its receiver (`fold_native_member`):
        // a string primitive's `charCodeAt` proves no object shape of it.
        if matches!(global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) { return None; }
        let FieldLookup::Atom { base, cache_site, .. } = instruction.field_lookup()? else { return None };
        let [field] = site_loads(fields, cache_site)?.as_slice() else { return None };
        field.own_direct().then_some((base.register_index()?, field.shape))
    };
    let kind_reads = super::super::body_views::kind_element_types(kinds, &instructions);
    let typed_seeds = super::super::facts::Seeds { typed_reads: &kind_reads, ..super::super::facts::Seeds::typed() };
    lowering.facts = super::super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &lowering.raises, typed_seeds);
    lowering.intrinsics = native_calls::intrinsic_calls(unit, global_reads, |pc| direct.contains_key(&pc));
    let loops = local_loops(unit, &program.constants, entry_pc, &lowering.intrinsics, global_reads);
    // One entry query of the call depth replaces each binding's helper with its cached
    // target probe (`cached_call_target`). A completed call leaves the depth it found.
    if !numeric && unit.frames.iter().any(|frame| frame.target.callee == CalleeBinding::Function) {
        let helper = lowering.helpers.target(&mut lowering.builder, LaneHelper::CallBudget);
        lowering.call_budget = Some(emit_lane_helper_call(&mut lowering.builder, lowering.payload, helper, &[]));
    }
    if coverage.selected_entry() {
        lowering.written = lowering.facts.written(&instructions, &writes, count);
        // Backedge definitions choose a numeric entry signature. Conformance
        // proves the external input once; every subsequent edge carries the
        // same verified SSA view. This is an admission requirement, not a fact
        // assumed about the source program (a declared parameter type is checked
        // the same way: the solve's entry cell).
        let mut refined = false;
        for (input, carried) in initial.iter_mut().take(usize::from(unit.caller_registers)).zip(lowering.facts.backedge().iter().map(|ty| ty.machine())) {
            if (carried == Type::INT32 || carried.int_range().is_some() || number_domains::is_double(carried)) && *input != carried {
                *input = carried;
                refined = true;
            }
        }
        // An integer induction value can compare against an invariant integer
        // input directly. This selects a checked entry contract; a fractional
        // or wider bound resumes the original header with its value unchanged.
        let instruction = instructions[entry_pc];
        if !lowering.facts.backedge().is_empty() && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. }))
            && matches!(super::super::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                    Some(super::super::operations::NumberRelation::LessThan | super::super::operations::NumberRelation::LessEqual
                        | super::super::operations::NumberRelation::GreaterThan | super::super::operations::NumberRelation::GreaterEqual)) {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            if operands.iter().any(|operand| super::super::facts::operand_type(&program.constants, *operand,
                |register| lowering.facts.backedge().get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_int32()) {
                for register in operands.into_iter().filter_map(Operand::register_index)
                    .filter(|register| *register < unit.caller_registers && !lowering.written.contains(usize::from(*register))) {
                    if number_domains::is_double(initial[usize::from(register)]) {
                        initial[usize::from(register)] = Type::INT32;
                        refined = true;
                    }
                }
            }
        }
        if refined { lowering.facts = super::super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &lowering.raises, typed_seeds); }
        lowering.refine_inductive_inputs(&mut initial, &instructions, &leaders, &loops, &own_shape);
    }
    lowering.refine_loop_inputs(entry_pc, &initial, &instructions, &leaders, &loops, &own_shape);
    lowering.require_object_latches(&instructions, &leaders, &loops);
    for &pc in generic { lowering.facts.widen(pc); }
    if !numeric { lowering.entry = Some(lowering.entry_facts(&instructions, &loops)); }
    // A preliminary run of the facts over the arrays the loops index without their captured reads
    // (`loop_arrays` before the hoists): a typed or double view's store the run proves a Number store
    // changes no length, so it does not keep a loop's `length` from its entry (`invariant_reads`).
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders, lowering.views, &element_kind, &lowering.intrinsics, |pc| coverage.lowered(pc));
    let preliminary = lowering.prove_ranges(entry_pc, &initial, &instructions, &leaders, &own_shape, &loops);
    let quiet_store = |pc: usize| preliminary.numeric_store(pc) && lowering.typed_access(pc) == Some(super::super::facts::TypedAccess::Store);
    let hoists = invariant_reads(program, unit, &loops, &lowering.intrinsics, &quiet_store, global_reads);
    lowering.hoists = hoists;
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders, lowering.views, &element_kind, &lowering.intrinsics, |pc| coverage.lowered(pc));
    let arrays = std::mem::take(&mut lowering.loop_arrays);
    lowering.loop_arrays = arrays.into_iter().filter(|array| !lowering.string_base(array)).collect();
    lowering.loop_strings = loop_strings::loop_strings(unit, &loops, &lowering.intrinsics, |pc| coverage.lowered(pc));
    lowering.ranges = lowering.prove_ranges(entry_pc, &initial, &instructions, &leaders, &own_shape, &loops);
    lowering.arrays_fixed = lowering.arrays_stay_put();
    lowering.loop_fields = loop_fields(program, unit, &loops);
    for field in &lowering.loop_fields {
        lowering.hoists.entry(field.header).or_default().push(InvariantRead { pc: field.reads[0], owner: Some(field.owner), rooted: false, callee: None });
    }
    // Field admissions establish the actual counterpart types used to select
    // capture views. Callee reads/bindings must precede their inlined captures.
    for reads in lowering.hoists.values_mut() {
        reads.sort_by_key(|read| (matches!(crate::compiled::performed(&instructions[read.pc]), Some(quench_runtime::staged::Operation::LoadCapture { .. })),
            matches!(unit.origins[read.pc], super::super::lane_unit::Origin::Inlined { .. })));
    }
    (lowering.forward, lowering.sealable) = edges::leader_edges(&instructions, &leaders, entry_pc);
    lowering.loop_carry = loop_carries(&instructions, &leaders, entry_pc);
    // No forward edge of the bytecode's enters a handler's block: its proofs are none, and it
    // seals once the region ends.
    for (pc, catch) in lowering.catches.iter().enumerate().filter_map(|(pc, catch)| Some((pc, (*catch)?))) {
        lowering.forward[catch.target] = None;
        lowering.sealable.remove(catch.target);
        // A throw of an inlined callee reaches its caller's handler block after the walk passed
        // it: the block is lowered when the walk reaches it, as one an edge entered.
        if catch.target < pc { lowering.entered.insert(catch.target); }
    }
    lowering.entered.insert(entry_pc);
    lowering.stable.start = entry_pc;
    let mut proven_types: Box<[Type]> = Box::default();
    {
        let pc = entry_pc;
        let block = lowering.builder.create_block();
        lowering.goto(block, &[]);
        lowering.builder.switch_to_block(block);
        lowering.begin_trips();
        if !matches!(lowering.unit.origins.get(pc), Some(super::super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let undefined = lowering.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        for register in 0..count {
            // A numeric entry's window holds its parameters alone; a scalar entry has none.
            let in_window = match completion {
                Completion::Scalar(_) => false,
                Completion::Number => register < usize::from(function.params),
                Completion::Body => register < usize::from(lowering.unit.caller_registers) && lowering.is_live(pc, register as Register),
            };
            let bits = if in_window {
                lowering.raw_load(Lane::I64, regions::frame(), registers_base, register_offset(register as Register))
            } else {
                undefined
            };
            lowering.builder.def_var(lowering.registers.tagged[register], bits);
        }
        let target = lowering.blocks.get(pc).copied().flatten()?;
        let mut state = State::canonical(count);
        if let Some(budget) = lowering.call_budget {
            // Every body edge is dominated by this admission. The closed region
            // cannot change thread call depth before leaving to materialize.
            let depth = lowering.unit.recovery_depth().max(1) as i64;
            let admitted = lowering.raw_compare_imm(Relation::UnsignedGreaterEqual, budget, depth);
            let miss = lowering.guard_exit(&state, pc as u32);
            emit_js_guard(&mut lowering.builder, admitted, miss);
        }
        if numeric { lowering.enter_numeric_parameters(&mut state, pc, &parameters); }
        if fault_poll {
            // Publication lends a resumable page for this entire no-GC entry.
            // If unavailable, leave before any guest effect through ordinary recovery.
            let miss = lowering.guard_exit(&state, pc as u32);
            let available = lowering.raw_compare_imm(Relation::NotEqual, interrupt, 0);
            emit_js_guard(&mut lowering.builder, available, miss);
        }
        // A proven entry's caller proved each argument has its parameter's entry type (`call_frames::call_target`), so the
        // guards those types make are not run: the state holds them, and the entry conform proves them with no test. A
        // shape is not held this way: its guard runs in every entry.
        if lowering.vm.proven_entry {
            let entered: Vec<Type> = lowering.facts.at(pc).map(|types| types.iter().copied().take(usize::from(function.params)).collect()).unwrap_or_default();
            for (index, ty) in entered.iter().copied().enumerate() {
                if !lowering.is_live(pc, index as Register) || ty.object_shape().is_some() { continue; }
                state.values[index] = RegisterFact { tagged: state.values[index].tagged, ..RegisterFact::definition(ty, Representation::Tagged) };
            }
            proven_types = entered.into_boxed_slice();
        }
        lowering.narrow_to_cells(&mut state, pc);
        lowering.entry_conform(&mut state, pc, coverage);
        // A region whose sole entry is this loop header has no earlier source
        // edge on which to hoist its invariant reads. Admit them on the native
        // entry edge, before the header and every possible loop effect. A failed
        // entry proof declines the region (the lane runs the header itself), not
        // an exit: the old lowering's entry admission declines the same arrays.
        // A bounded range reads its fields at their sites; hoisting them to a shared entry
        // guard would make every call leave at the entry.
        lowering.proving_entry = true;
        lowering.hoist_entry(&mut state, pc);
        lowering.proving_entry = false;
        lowering.goto(target, &[]);
    }
    let summarized = completion == Completion::Body && matches!(coverage, Coverage::Prefix);
    let mut census = clif_census::Census::new();
    let emitted = |lowering: &Lowering<'_, '_>| lowering.builder.func.dfg.num_insts();
    let mut mark = emitted(&lowering);
    let mut block_mark = lowering.builder.func.dfg.num_blocks();
    if let Some(census) = census.as_mut() { census.part(clif_census::Part::Entry, mark); }
    let mut state = State::canonical(count);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if !coverage.lowered(pc) { continue; }
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.hoist_entry(&mut state, pc);
                lowering.conform(&mut state, pc);
                lowering.carry_kept(&mut state, pc);
                lowering.arrive(&state, pc);
                lowering.entered.insert(pc);
                lowering.goto(block, &[]);
            }
            // Marker alternatives add real native edges beyond the bytecode
            // analysis's Nop fallthrough. Missing type facts select canonical
            // guarded lowering; they do not make an entered block unreachable.
            if !lowering.entered.contains(pc) {
                reachable = false;
                continue;
            }
            lowering.switch_to(block, lowering.sealable.contains(pc));
            state = lowering.entry_state(pc, count);
            lowering.join_virtuals(&mut state, pc);
            lowering.keep_shared_proofs(&mut state, pc);
            lowering.narrow_to_cells(&mut state, pc);
            lowering.carry_typed_views(&mut state, pc);
            lowering.seed_loop_shapes(&mut state, pc);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        lowering.at = pc;
        if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Edges, now - mark); mark = now; block_mark = lowering.builder.func.dfg.num_blocks(); }
        reachable = lowering.instruction(&mut state, pc, instruction);
        lowering.retire_fresh(&mut state, pc, instruction);
        if let Some(census) = census.as_mut() {
            let now = emitted(&lowering);
            if census.sample(instruction.op()) {
                let insts: Vec<String> = (mark..now).map(|index| format!("{}", lowering.builder.func.dfg.display_inst(ir::Inst::new(index)))).collect();
                eprintln!("QUENCH_CLIF_SAMPLE {:?} pc={pc} n={}\n  {}", instruction.op(), now - mark, insts.join("\n  "));
            }
            census.op(instruction.op(), now - mark); mark = now;
            let blocks = lowering.builder.func.dfg.num_blocks();
            census.blocks(instruction.op(), blocks - block_mark); block_mark = blocks;
        }
    }
    if reachable {
        lowering.exit(&mut state, coverage.exit_pc(instructions.len()) as u32);
    }
    for (header, latch) in std::mem::take(&mut lowering.poll_latches).iter() {
        if let Some(latch) = latch {
            lowering.builder.switch_to_block(*latch);
            lowering.emit_fault_poll();
            lowering.goto(header, &[]);
        }
    }
    // A prefix pays for its entry with a loop, or with the calls it runs inlined.
    if matches!(coverage, Coverage::PrefixFrom(_)) && !lowering.backedge && unit.frames.is_empty() { return None; }
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Tail, now - mark); mark = now; }
    let named = lowering.named_leaves();
    for GuardExit { block, values, homed, virtuals, virtual_arrays, pc, line, .. } in lowering.take_leaves() {
        lowering.switch_to(block, true);
        if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            eprintln!("aot-lanes: function {function_id}: a leave at {pc} (lowering line {line})");
        }
        let mut leaving = State { values, proofs: LocalProofs { homed, ..LocalProofs::default() }, refinements: smallvec::SmallVec::new(), virtuals, virtual_arrays };
        lowering.record_line(line);
        lowering.leave_at(&mut leaving, pc);
    }
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Leaves, now - mark); mark = now; }
    lowering.fill_call_out_stop();
    if lowering.fill_exits().is_none() { declined("an exit fill declines", usize::MAX); return None; }
    let exits = ExitTable::of_unit(unit, |pc, frame| lowering.inlined_stores(pc, frame), |pc| named.get(pc).copied().flatten());
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Exits, now - mark); mark = now; }
    lowering.builder.switch_to_block(stop);
    let word = lowering.builder.block_params(stop)[0];
    let status = lowering.raw_const(Lane::I32, i64::from(NATIVE_JS_STOPPED));
    lowering.leave_with(status, Some(word));
    lowering.builder.switch_to_block(leave);
    let results = lowering.builder.block_params(leave).to_vec();
    lowering.flush_trips();
    lowering.leave(&results);
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Tail, now - mark); census.report(function_id, now);
        let mut opcodes: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for index in 0..now { *opcodes.entry(format!("{:?}", lowering.builder.func.dfg.insts[ir::Inst::new(index)].opcode())).or_default() += 1; }
        let blocks = lowering.builder.func.layout.blocks().count();
        let cold = lowering.builder.func.layout.blocks().filter(|block| lowering.builder.func.layout.is_cold(*block)).count();
        eprintln!("QUENCH_CLIF_BLOCKS {blocks} {cold}");
        eprintln!("QUENCH_CLIF_OPCODES {}", opcodes.iter().map(|(name, count)| format!("{name}={count}")).collect::<Vec<_>>().join(" ")); }
    // A lane body is a whole function body and must complete; so must an inlined unit, whose
    // callees rejoin it. A loop or range over the function's own code completes by its exits
    // and backedge, which the plan emits where the interpreter resumes.
    let must_finish = match coverage {
        Coverage::Prefix | Coverage::PrefixFrom(_) | Coverage::Full => true,
        Coverage::Loop { .. } => !unit.frames.is_empty(),
    };
    if !lowering.valid || (completion.status_and_pc() && must_finish && !lowering.finishes) {
        if lowering.valid { declined("the body does not finish", usize::MAX); }
        return None;
    }
    let number_result = numeric.then(|| lowering.finish_number_returns());
    let summary = summarized.then(|| TypeSummary);
    let native_count = lowering.native;
    let hoists_arrays = !lowering.loop_arrays.is_empty();
    let mut polls = std::mem::take(&mut lowering.polls);
    let reentries = std::mem::take(&mut lowering.reentries);
    let stable = lowering.stable.take();
    let field_regions = std::mem::take(&mut lowering.field_regions);
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    super::super::regions::retag_field_regions(&mut native, &field_regions);
    super::super::jit_backend::builder::keep_context(builder_context);
    let decided = known_branches::decide(&mut native);
    if clif_census::enabled() { eprintln!("QUENCH_CLIF_DECIDED {decided}"); }
    if let Some(owner) = interrupt_owner { specialize_closed_polls(&mut native, &reentries, &mut polls, owner); }
    if naming.folded() { declined("a slow path no helper names", usize::MAX); return None; }
    Some(LoweredRegion { function: native, native: native_count, fault_poll, number_result, hoists_arrays, stable, summary, entry_types: proven_types, exits })
}

