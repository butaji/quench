//! Register roles of admission: the pass that assigns each register of a loop region its role.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn admit_register_roles(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    header_live: Option<RegisterSet>,
    header: u32,
    objects: &std::collections::BTreeSet<Register>,
    method_calls: MethodCalls,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    let backedge = header + instructions.len() as u32 - 1;
    // A held register keeps the value its path read at entry, which a store to any name on
    // the path could change: a path through a name the region stores is not held.
    let stored_names: std::collections::BTreeSet<quench_runtime::bytecode::Atom> = instructions
        .iter()
        .filter(|instruction| admitted_store(**instruction))
        .map(|instruction| instruction.atom_index())
        .collect();
    let rooted: std::collections::BTreeMap<Register, FieldPath> = held_registers(&instructions, sites, header_live, objects)
        .into_iter()
        .filter(|(_, path)| path.steps.iter().all(|(atom, _)| !stored_names.contains(atom)))
        .collect();
    let paths: std::collections::BTreeMap<Register, FieldPath> = rooted.iter().filter(|(_, path)| !path.steps.is_empty()).map(|(register, path)| (*register, path.clone())).collect();
    let dynamic_arrays = dynamic_arrays(&instructions, &paths, header)?;
    let mut tagged = match tagged_registers_with_failure(
        &instructions,
        constants,
        &objects.iter().chain(paths.keys()).copied().collect(),
        &dynamic_arrays,
        header,
    ) {
        Ok(tagged) => tagged,
        Err(failure_offset) => {
            if quench_runtime::trace::Channel::OsrReject.on() {
                for (offset, instruction) in instructions.iter().copied().enumerate().filter(|(_, instruction)| {
                    quench_runtime::staged::compare_jump(instruction.op())
                }) {
                    let operand = |operand: Operand| {
                        let constant = (operand.kind() == Some(OperandKind::Constant))
                            .then(|| constants.get(usize::from(operand.payload())).copied());
                        (operand.kind(), operand.payload(), constant.flatten())
                    };
                    eprintln!(
                        "QUENCH_JS_JIT_OSR_TAGGED_REJECT header={header} offset={offset} binary_operator={} equality={:?} number_relation={:?} operand_b={:?} operand_c={:?}",
                        instruction.binary_operator_field(),
                        quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field()),
                        quench_runtime::vm::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                        operand(instruction.operand_b()),
                        operand(instruction.operand_c()),
                    );
                }
            }
            return Err(RegionAnalysisFailure::at(
                RegionRejection::UnsupportedInstruction(instructions[failure_offset].op() as u16),
                header,
                failure_offset,
            ));
        }
    };
    tagged.extend(paths.keys().copied());
    let mut admission = Admission {
        constants,
        boxed: boxed(&instructions),
        roles: std::collections::BTreeMap::new(),
        captures: Vec::new(),
        fields: Vec::new(),
        numbers: std::collections::BTreeMap::new(),
        values: std::collections::BTreeMap::new(),
        field_arrays: std::collections::BTreeMap::new(),
    };
    let mut field_reads = std::collections::BTreeMap::new();
    let (mut dynamic_field_reads, mut field_stores) = (std::collections::BTreeMap::new(), std::collections::BTreeMap::new());
    // The names the region stores on a receiver it never rewrites (`this`, or an unchanged
    // register): a read of one through that receiver reads its current slot at the read.
    let written: std::collections::BTreeSet<Register> =
        instructions.iter().filter_map(|instruction| instruction.defined_register()).collect();
    let stored_fixed: std::collections::BTreeSet<(quench_runtime::bytecode::Atom, FieldReceiver)> = instructions
        .iter()
        .filter(|instruction| admitted_store(**instruction))
        .filter_map(|instruction| match instruction.field_store_register() {
            None => Some((instruction.atom_index(), FieldReceiver::This)),
            Some(receiver) if !written.contains(&receiver) => Some((instruction.atom_index(), FieldReceiver::Register(receiver))),
            Some(_) => None,
        })
        .collect();
    // A call the region continues past may write any field, so a read through a receiver the
    // region never rewrites reads its current slot, as one of the region's own stored names does.
    let calls = instructions.iter().any(|instruction| quench_runtime::staged::calls_ordinary(instruction.op()));
    let mut dynamic_field_stores = std::collections::BTreeMap::new();
    let mut field_store_sites = Vec::new();
    let defined_registers: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| instruction.defined_register())
        .collect();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        if quench_runtime::staged::reads_named_field(instruction.op()) {
            let path = field_path(instruction, sites, &rooted).ok_or_else(|| {
                RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op() as u16),
                    header,
                    offset,
                )
            })?;
            let index = intern_field(&mut admission.fields, &path, tagged.contains(&instruction.result_register()));
            field_reads.insert(offset as u32, index);
            if path.steps.len() == 1
                && let FieldReceiver::Register(receiver) = path.root
                && defined_registers.contains(&receiver)
                // A field result used as an array base is entry-pinned separately by the
                // array lowering, so leave that combination on the conservative path.
                && !instructions.iter().any(|candidate| {
                    quench_runtime::staged::indexes_receiver(candidate.op())
                        && object_reads(*candidate) == Some(instruction.result_register())
                })
            {
                dynamic_field_reads.insert(offset as u32, index);
            } else if path.steps.len() == 1 && (calls || stored_fixed.contains(&(path.steps[0].0, path.root))) {
                dynamic_field_reads.insert(offset as u32, index);
                if path.root == FieldReceiver::This && !admission.captures.contains(&THIS_CAPTURE) {
                    admission.captures.push(THIS_CAPTURE);
                }
            }
        }
        if admitted_store(instruction) {
            if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                return Err(RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op() as u16),
                    header,
                    offset,
                ));
            }
            let receiver = instruction.field_store_register().map_or(FieldReceiver::This, FieldReceiver::Register);
            let atom = instruction.atom_index();
            let cache_site = instruction.cache_site_index();
            // A receiver the region redefines stores through the VM's cached store at the
            // instruction; every read of its name must then read the current slot too.
            if let FieldReceiver::Register(register) = receiver
                && defined_registers.contains(&register)
            {
                let current = |candidate: &WideInstruction| {
                    field_path(*candidate, sites, &rooted).is_some_and(|path| {
                        path.steps.len() == 1
                            && matches!(path.root, FieldReceiver::Register(register) if defined_registers.contains(&register))
                    })
                };
                if instructions.iter().any(|candidate| reads_name(*candidate, sites, atom) && !current(candidate)) {
                    return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, offset));
                }
                dynamic_field_stores.insert(offset as u32, (atom, cache_site));
            } else {
                // The slot pointer is captured at entry, of a receiver the region never
                // rewrites. A read of the name through that receiver reads the current slot
                // (`stored_fixed`), as does one through another receiver the region stores the
                // name on, which may be the same object; any other read of it could alias and
                // keep a stale value.
                if instructions.iter().any(|candidate| {
                    reads_name(*candidate, sites, atom)
                        && !field_path(*candidate, sites, &rooted).is_some_and(|path| path.steps.len() == 1
                            && (path.root == receiver || stored_fixed.contains(&(atom, path.root))))
                }) {
                    return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, offset));
                }
                let site = FieldStore { receiver, atom, cache_site };
                let index = field_store_sites.iter().position(|existing| *existing == site).unwrap_or_else(|| {
                    field_store_sites.push(site);
                    field_store_sites.len() - 1
                });
                field_stores.insert(offset as u32, index);
            }
        }
        // A field-resolved method call is the inliner's (`MethodCalls::inlined`), unless the
        // inliner declined it. Dynamic/direct calls have no such owner.
        if method_calls.inlined(objects) && quench_runtime::staged::calls_ordinary(instruction.op()) {
            let callee_register = instruction.register_b();
            if instructions[..offset]
                .iter()
                .rev()
                .find(|candidate| candidate.defined_register() == Some(callee_register))
                .is_some_and(|definition| quench_runtime::staged::reads_named_field(definition.op()))
            {
                return Err(RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op() as u16),
                    header,
                    offset,
                ));
            }
        }
        if let Some(register) = object_reads(instruction).filter(|register| paths.contains_key(register)) {
            admission.field_arrays.entry(register).or_insert(false);
        }
        if admit(&mut admission, instruction, constants, &tagged, &defined_registers, &dynamic_arrays).is_none() {
            if quench_runtime::trace::Channel::OsrReject.on() {
                let constant = (quench_runtime::staged::loads_constant(instruction.op()))
                    .then(|| constants.get(instruction.constant_index()).copied());
                let result = instruction.defined_register();
                eprintln!(
                    "QUENCH_JS_JIT_OSR_REJECT header={header} offset={offset} op={:?} a={} b={} c={} imm={} constants_len={} result={result:?} tagged_result={} result_role={:?} constant={:?}",
                    instruction.op(),
                    instruction.a(),
                    instruction.b(),
                    instruction.c(),
                    instruction.imm(),
                    constants.len(),
                    result.is_some_and(|register| tagged.contains(&register)),
                    result.and_then(|register| admission.roles.get(&register)),
                    constant.flatten(),
                );
            }
            return Err(RegionAnalysisFailure::at(
                RegionRejection::UnsupportedInstruction(instruction.op() as u16),
                header,
                offset,
            ));
        }
    }
    let held: std::collections::BTreeMap<Register, (FieldPath, usize)> = paths
        .into_iter()
        .map(|(register, path)| {
            let index = intern_field(&mut admission.fields, &path, true);
            (register, (path, index))
        })
        .collect();
    // A mutable field is one own-data Number cell. Every same-name read must
    // name that exact unchanged receiver; a different receiver may alias it.
    // Its stores must remain Numbers, so no write can invalidate its view or
    // require a reference barrier. Nested/tagged paths retain snapshot semantics.
    for (offset, index) in &field_stores {
        let store = &field_store_sites[*index];
        // A read through another receiver the name is stored on is that store's to check.
        let reads = admission.fields.iter().filter(|field| field.atom == store.atom
            && (field.receiver == store.receiver || !stored_fixed.contains(&(field.atom, field.receiver))));
        for field in reads {
            if !store.matches(field) || field.tagged
                || admission.roles.get(&instructions[*offset as usize].register_a()) != Some(&Role::Scalar(Representation::Float64))
            {
                return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, *offset as usize));
            }
        }
    }
    let mut identity_branches = std::collections::BTreeMap::new();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        let Some(branch) = identity_branch(instruction, constants) else {
            continue;
        };
        identity_branches.insert(header + offset as u32, branch);
    }
    if admission.fields.len() > MAX_REGION_FIELDS {
        return Err(RegionAnalysisFailure { reason: RegionRejection::FieldLimit, pc: backedge });
    }
    if field_store_sites.len() > MAX_REGION_FIELD_STORES {
        return Err(RegionAnalysisFailure { reason: RegionRejection::FieldStoreLimit, pc: backedge });
    }
    // Entry-snapshot reads require a stable receiver. Direct one-step reads can instead
    // probe the current receiver at each instruction; nested paths, held paths, and
    // field-backed arrays retain the entry-snapshot contract.
    if let Some((field, write_offset)) = admission.fields.iter().enumerate().find_map(|(index, field)| {
        let FieldReceiver::Register(register) = field.receiver else { return None };
        let write_offset = instructions
            .iter()
            .position(|instruction| instruction.defined_register() == Some(register))?;
        let uses: Vec<u32> = field_reads
            .iter()
            .filter_map(|(offset, used)| (*used == index).then_some(*offset))
            .collect();
        (!uses.is_empty() && uses.iter().any(|offset| !dynamic_field_reads.contains_key(offset)))
            .then_some((field, write_offset))
    }) {
        if quench_runtime::trace::Channel::OsrReject.on() {
            eprintln!(
                "QUENCH_JS_JIT_OSR_FIELD_REJECT header={header} reason=read_receiver_redefined atom={} receiver={:?} write_offset={write_offset}",
                field.atom,
                field.receiver,
            );
        }
        return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, write_offset));
    }
    if admission.roles.keys().any(|register| usize::from(*register) >= registers)
        || admission.roles.len() > MAX_REGION_REGISTERS
    {
        return Err(RegionAnalysisFailure { reason: RegionRejection::RegisterLimit, pc: backedge });
    }
    if admission.captures.len() > MAX_REGION_CAPTURES {
        return Err(RegionAnalysisFailure { reason: RegionRejection::CaptureLimit, pc: backedge });
    }
    let numbers = admission
        .roles
        .iter()
        .filter(|(_, role)| **role == Role::Scalar(Representation::Float64))
        .map(|(register, _)| *register)
        .collect();
    let tagged: Vec<Register> = admission
        .roles
        .iter()
        .filter(|(_, role)| matches!(role, Role::Scalar(Representation::Tagged) | Role::Array { elements: Representation::Tagged, .. }))
        .map(|(register, _)| *register)
        .collect();
    let mut arrays: Vec<ArrayUse> = admission
        .roles
        .iter()
        .filter_map(|(register, role)| match role {
            Role::Array { written, .. }
                if !dynamic_arrays.contains(register)
                    && instructions.iter().any(|instruction| {
                    quench_runtime::staged::indexes_receiver(instruction.op())
                        && object_reads(*instruction) == Some(*register)
                }) => Some(ArrayUse { register: *register, written: *written, field: None }),
            Role::Scalar(_) | Role::Array { .. } => None,
        })
        .collect();
    for (register, written) in &admission.field_arrays {
        // A held register read as an object only by field reads indexes nothing.
        if !instructions.iter().any(|instruction| {
            quench_runtime::staged::indexes_receiver(instruction.op())
                && object_reads(*instruction) == Some(*register)
        }) {
            continue;
        }
        arrays.push(ArrayUse { register: *register, written: *written, field: Some(held[register].1) });
    }
    if arrays.len() > MAX_REGION_ARRAYS {
        return Err(RegionAnalysisFailure { reason: RegionRejection::ArrayLimit, pc: backedge });
    }
    let stored: std::collections::BTreeSet<u16> = instructions.iter()
        .filter(|instruction| matches!(Operation::<(), ()>::decode(**instruction, DecodeContext::EMPTY), Some(Operation::StoreCapture { .. })) && instruction.capture_depth() == 0)
        .map(|instruction| instruction.capture_slot())
        .collect();
    let (stored_captures, captures) = admission.captures.into_iter().partition(|slot| stored.contains(slot));
    Ok(LoopRegion {
        stencil_only: false,
        header,
        backedge,
        numbers,
        tagged,
        arrays,
        captures,
        stored_captures,
        fields: admission.fields,
        field_reads,
        dynamic_field_reads,
        dynamic_arrays,
        field_stores,
        dynamic_field_stores,
        field_store_sites,
        held,
        identity_branches,
        instructions,
        constants: admission.numbers,

        live,
        inlining: None,
    })
}
