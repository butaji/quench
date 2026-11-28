use super::*;

/// Per block leader that a later PC enters (a loop's header, or a rotated body a latch enters), its forward edges and
/// the last PC that enters it (`Lowering::carry_typed_views`). The entry `start` has edges no instruction names: none.
pub(super) fn loop_carries(instructions: &[WideInstruction], leaders: &BitSet, start: usize) -> BTreeMap<usize, (usize, usize)> {
    let mut edges: BTreeMap<usize, (usize, Option<usize>)> = BTreeMap::new();
    let mut add = |pc: usize, target: usize| {
        if !leaders.contains(target) || target == start { return; }
        let entry = edges.entry(target).or_insert((0, None));
        if target > pc { entry.0 += 1 } else { entry.1 = Some(entry.1.map_or(pc, |last: usize| last.max(pc))) }
    };
    for (pc, instruction) in instructions.iter().enumerate() {
        quench_runtime::bytecode::for_each_instruction_successor(pc, *instruction, |target| add(pc, target));
    }
    // A latch whose loop's header is a test repeats it (`edges::rotated_bodies`): its edges are the test's, from the latch.
    for (latch, header) in quench_runtime::bytecode::back_edges_of(instructions.iter().copied()) {
        let Some(&test) = instructions.get(header) else { continue };
        if !quench_runtime::staged::meaning::means(test, quench_runtime::staged::meaning::Meaning::ConditionalJump) { continue; }
        quench_runtime::bytecode::for_each_instruction_successor(header, test, |target| add(latch, target));
    }
    edges.into_iter().filter_map(|(pc, (forward, last))| Some((pc, (forward, last?)))).collect()
}

/// Loops with one external entry and no interior entry, whose native operations
/// only read or replace existing own fields and array elements. Misses leave before guest effects.
/// A sole native entry at the header can supply the same entry proofs.
pub(super) fn local_loops(unit: &super::super::lane_unit::Unit, constants: &[quench_runtime::bytecode::Constant], start: usize,
    intrinsics: &native_calls::Intrinsics, global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> Vec<(usize, usize)> {
    let instructions = &unit.instructions;
    let target = |instruction: &WideInstruction| quench_runtime::bytecode::jump_edge(*instruction);
    let mut loops = Vec::new();
    for (back, header) in quench_runtime::bytecode::back_edges_of(instructions.iter().copied()) {
        let entry_header = start == header;
        let body = &instructions[header..=back];
        let entries = usize::from(header > 0 && quench_runtime::bytecode::falls_into_next(header - 1, instructions[header - 1]))
            + instructions[..header].iter().filter(|candidate| target(candidate) == Some(header)).count();
        let side = instructions.iter().enumerate().any(|(pc, candidate)| !(header..=back).contains(&pc)
            && target(candidate).is_some_and(|to| (header + 1..=back).contains(&to) || (to == header && pc > back)));
        if (entries != 1 && !entry_header) || side
            || (header + 1..=back).contains(&start)
            || !body.iter().enumerate().all(|(offset, candidate)| {
                let pc = header + offset;
                stays_native(constants, *candidate, native_calls::holds_in_loop(intrinsics, pc) || native_calls::scalar_name_read(global_reads, pc))
            }) { continue }
        loops.push((header, back));
    }
    loops
}

pub(super) fn invariant_reads(program: &ResidualProgram, unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)],
    intrinsics: &native_calls::Intrinsics, quiet_store: &dyn Fn(usize) -> bool,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> BTreeMap<usize, Vec<InvariantRead>> {
    let mut hoists = BTreeMap::new();
    let boundaries = leaders(&unit.instructions);
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let stored: BTreeSet<Atom> = body.iter().filter(|candidate| stores_own(**candidate))
            .map(|candidate| candidate.atom_index()).collect();
        // An element store, an append or a pop may change its array's `length`; a store
        // `quiet_store` proves through a typed view changes none.
        let resizes = body.iter().enumerate().any(|(offset, candidate)| {
            let pc = header + offset;
            (stores_element(*candidate) && !quiet_store(pc)) || native_calls::resizes(intrinsics, pc)
        });
        let mut reads: Vec<InvariantRead> = body.iter().enumerate().filter_map(|(offset, candidate)| {
            let pc = header + offset;
            if matches!(crate::compiled::performed(candidate), Some(quench_runtime::staged::Operation::LoadCapture { .. }))
                && matches!(unit.origins[pc], super::super::lane_unit::Origin::Caller(_)) {
                return Some(InvariantRead { pc, owner: None, rooted: stored.is_empty(), callee: None });
            }
            if !matches!(crate::compiled::performed(candidate), Some(quench_runtime::staged::Operation::GetField { .. })) { return None; }
            // A native method's read folds to the function its holder holds (`fold_native_member`): its receiver,
            // a string primitive for `charCodeAt`, is no object whose field the entry would hoist.
            if matches!(global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) { return None; }
            match candidate.field_lookup() {
            Some(FieldLookup::Atom { atom, base, .. }) if !stored.contains(&atom) && !(resizes && names_length(program, atom)) => {
                let owner = invariant_field_owner(unit, header, pc, base.register_index()?, &written, &boundaries)?;
                Some(InvariantRead { pc, owner: Some(owner), rooted: false, callee: None })
            }
            _ => None,
        }}).collect();
        // An inlined capture belongs to the actual closure admitted at its
        // marker, not merely to a function body or an observed environment.
        // Its invariant callee read must run first on the entry edge.
        let captures: Vec<_> = body.iter().enumerate().filter_map(|(offset, instruction)| {
            let pc = header + offset;
            let super::super::lane_unit::Origin::Inlined { frame, .. } = unit.origins[pc] else { return None };
            if !matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) { return None; }
            let read = invariant_callee_read(unit, reads.iter().map(|read| read.pc), frame, &boundaries).is_some();
            // A function-bound callee the loop neither writes nor reads again is held at entry.
            let callee = (!read).then(|| entry_callee(unit, frame, &written, body)).flatten();
            (read || callee.is_some()).then_some(InvariantRead { pc, owner: None, rooted: false, callee })
        }).collect();
        reads.extend(captures);
        if !reads.is_empty() { hoists.insert(header, reads); }
    }
    hoists
}

/// Resolve only block-local receiver copies to a caller home unchanged by the
/// whole loop. A branch or implicit frame initialization ends this proof.
pub(super) fn invariant_field_owner(unit: &super::super::lane_unit::Unit, header: usize, pc: usize,
    mut owner: Register, written: &BTreeSet<Register>, boundaries: &BitSet) -> Option<Register> {
    for at in (header..pc).rev() {
        if owner < unit.caller_registers && !written.contains(&owner) { return Some(owner); }
        let instruction = unit.instructions[at];
        if boundaries.contains(at + 1) || unit.markers.contains(&at)
            || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return None; }
        if writes(instruction).contains(&owner) {
            if !matches!(super::super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_register() != owner { return None; }
            owner = instruction.register_b();
        }
    }
    (owner < unit.caller_registers && !written.contains(&owner)).then_some(owner)
}

/// The caller register a function-bound inlined frame's closure sits in, where the loop writes
/// it nowhere and stores no capture: the closure it holds at loop entry is every iteration's
/// callee, so its captures are read once on the entry edge (`Lowering::admitted_callee_read`).
/// `None` for an identity-bound or alternative frame, or a callee the loop may change.
pub(super) fn entry_callee(unit: &super::super::lane_unit::Unit, frame: usize, written: &BTreeSet<Register>,
    body: &[WideInstruction]) -> Option<Register> {
    let call = unit.frames[frame];
    let marker = unit.markers[frame];
    let register = call.callee_register;
    (call.target.callee == CalleeBinding::Function && call.alternative.is_none()
        && !unit.frames.iter().any(|other| other.alternative == Some(marker))
        && register < unit.caller_registers && !written.contains(&register)
        && !body.iter().any(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::StoreCapture { .. }))))
        .then_some(register)
}

/// A monomorphic frame whose callee comes from an entry read through only
/// block-local copies. Other markers implicitly initialize scratch homes.
pub(super) fn invariant_callee_read(unit: &super::super::lane_unit::Unit, reads: impl IntoIterator<Item = usize>, frame: usize,
    boundaries: &BitSet) -> Option<usize> {
    let call = unit.frames[frame];
    let marker = unit.markers[frame];
    if call.alternative.is_some() || unit.frames.iter().any(|other| other.alternative == Some(marker)) {
        return None;
    }
    reads.into_iter().find(|&pc| {
        if pc >= marker || (pc + 1..=marker).any(|at| boundaries.contains(at)) { return false; }
        let mut source = call.callee_register;
        for at in (pc + 1..marker).rev() {
            let instruction = unit.instructions[at];
            if unit.markers.contains(&at)
                || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&source) {
                if !matches!(super::super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_register() != source { return false; }
                source = instruction.register_b();
            }
        }
        unit.instructions[pc].result_register() == source
    })
}

#[cfg(test)]
pub(super) fn record_field_forward(cache_site: u16) {
    FIELD_VALUE_FORWARDS.with(|count| count.set(count.get() + 1));
    FIELD_FORWARD_SITE.with(|site| {
        if let Some((expected, count)) = site.get() && expected == cache_site {
            site.set(Some((expected, count + 1)));
        }
    });
}

/// One authoritative alias plan; lowering derives entry probes and typed SSA
/// homes from it. Different stable roots are not proofs of disjoint objects.
pub(super) struct LoopField {
    pub(super) header: usize,
    pub(super) owner: Register,
    pub(super) atom: Atom,
    pub(super) reads: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]>,
    pub(super) stores: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]>,
    /// Conditional on this field's packed/no-extras entry admission.
    pub(super) store_ranges: smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]>,
}

/// Forward must-alias analysis through inline argument copies and branch joins.
/// Only a single backedge is admitted, so every incoming state is complete when
/// visited. Unknown writes decline promotion; no runtime alias cache is needed.
pub(super) fn loop_fields(program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
    loops: &[(usize, usize)]) -> Vec<LoopField> {
    let mut fields: Vec<LoopField> = Vec::new();
    for &(header, back) in loops {
        if loops.iter().any(|&(other, end)| (other, end) != (header, back) && other <= back && header <= end)
            || (header..=back).any(|pc| {
                let mut back_edge = false;
                quench_runtime::bytecode::for_each_instruction_successor(pc, unit.instructions[pc], |target| back_edge |= target <= pc && (pc, target) != (back, header));
                back_edge
            })
            || unit.markers.iter().zip(&unit.frames).any(|(marker, frame)|
                (header..=back).contains(marker) && frame.alternative.is_some()) { continue; }
        let mut written: BitSet = BitSet::with_capacity(usize::from(unit.registers));
        for instruction in &unit.instructions[header..=back] {
            for register in writes(*instruction) { written.insert(usize::from(register)); }
        }
        let roots: Vec<Option<Register>> = (0..unit.registers)
            .map(|register| (register < unit.caller_registers && !written.contains(usize::from(register))).then_some(register)).collect();
        let mut incoming = vec![None; back - header + 1];
        incoming[0] = Some(roots);
        let first_field = fields.len();
        let mut stores = Vec::new();
        for pc in header..=back {
            let Some(mut aliases) = incoming[pc - header].take() else { continue };
            if let Some(frame) = unit.markers.iter().position(|marker| *marker == pc).map(|index| unit.frames[index]) {
                for register in frame.base..frame.base + frame.registers { aliases[usize::from(register)] = None; }
                for (destination, source) in frame.passed_registers(program.function(frame.target.function as usize)) {
                    aliases[usize::from(destination)] = aliases[usize::from(source)];
                }
            }
            let instruction = unit.instructions[pc];
            if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && instruction.result_flags() == 0
                && let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
                && let Some(owner) = base.register_index().and_then(|register| aliases[usize::from(register)]) {
                if let Some(field) = fields[first_field..].iter_mut().find(|field| field.owner == owner && field.atom == atom) {
                    field.reads.push(pc);
                } else {
                    fields.push(LoopField { header, owner, atom, reads: smallvec::smallvec![pc],
                        stores: smallvec::SmallVec::new(), store_ranges: smallvec::SmallVec::new() });
                }
            }
            if stores_own(instruction) {
                let receiver = match instruction.field_store_register() {
                    Some(register) => Some(register),
                    None => match unit.origins[pc] {
                        super::super::lane_unit::Origin::Inlined { frame, .. } => Some(unit.frames[frame].receiver),
                        _ => None,
                    },
                };
                stores.push((receiver.and_then(|register| aliases[usize::from(register)]), instruction.atom_index(), pc));
            }
            let copied = (matches!(super::super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. }))).then(|| aliases[usize::from(instruction.register_b())]).flatten();
            for register in writes(instruction) { aliases[usize::from(register)] = None; }
            if matches!(super::super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) { aliases[usize::from(instruction.result_register())] = copied; }
            let mut forward_targets = smallvec::SmallVec::<[usize; 2]>::new();
            quench_runtime::bytecode::for_each_instruction_successor(pc, instruction, |target| if target > pc && target <= back { forward_targets.push(target) });
            let mut targets = forward_targets.into_iter().peekable();
            while let Some(target) = targets.next() {
                // Linear control transfers ownership of its flat state; only a
                // branch needs a second copy for the second successor.
                let incoming_roots = if targets.peek().is_some() { aliases.clone() } else { std::mem::take(&mut aliases) };
                let slot = &mut incoming[target - header];
                if let Some(known) = slot {
                    for (known, root) in known.iter_mut().zip(incoming_roots) {
                        if *known != root { *known = None; }
                    }
                } else { *slot = Some(incoming_roots); }
            }
        }
        for field in &mut fields[first_field..] {
            if stores.iter().any(|(owner, atom, _)| *atom == field.atom && *owner != Some(field.owner)) { continue; }
            field.stores.extend(stores.iter().filter_map(|(_, atom, pc)| (*atom == field.atom).then_some(*pc)));
        }
        fields.retain(|field| !field.stores.is_empty());
        for field in fields.iter_mut().filter(|field| field.header == header) {
            field.store_ranges = packed_ranges::analyze(program, unit, field, back);
        }
    }
    fields
}

