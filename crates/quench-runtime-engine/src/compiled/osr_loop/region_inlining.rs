//! Inlining a loop region's calls: the region inliner, which makes the inlined body of a region.
use super::*;

/// The region of the loop from `header` to `backedge` with each `Call` in it inlined: the
/// call's callee (`resolve` of the cached field it is read from) runs in place, its registers
/// above the caller's, when it is a function of plain instructions whose every parameter is
/// a Number argument or an object a caller register holds unchanged, and the whole region
/// then is one the tier lowers.
///
/// The call's setup, from its callee's read to the call, is a straight run of `pure_setup`
/// instructions writing no register live at the read: leaving there resumes at the read,
/// which reruns them. The read itself and the moves of the object arguments drop out, so the
/// registers they write, which the setup's function may reuse otherwise, hold nothing for
/// the region. Leaving at a callee's instruction makes the callee's activation
/// (`LaneExit::Inlined`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn inline_region<'a>(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: &quench_runtime::compile::liveness::RegisterMasks,
    header: u32,
    backedge: u32,
    resolve: &mut dyn FnMut(&[FieldUse], CalleeSource) -> Option<InlineCallee<'a>>,
) -> Option<LoopRegion> {
    let (start, end) = (header as usize, backedge as usize);
    (start <= end && end < decoded.len()).then_some(())?;
    let caller = &decoded[start..=end];
    let defines = |register: Register| caller.iter().any(|instruction| instruction.defined_register() == Some(register));
    let targets: std::collections::BTreeSet<u32> = caller
        .iter()
        .filter(|instruction| instruction.op().immediate_role() == ImmediateRole::JumpTarget)
        .map(|instruction| instruction.imm())
        .collect();
    let mask = |pc: u32| live.get(pc as usize);
    let caller_registers = function.registers;
    let mut total = u32::from(caller_registers);
    let mut calls: inlining::PcMap<inlining::Site<'a>> = inlining::PcMap::new(header, caller.len());
    // The caller PCs whose instructions drop out, the setup PCs by the PC they resume at, and
    // the caller instructions the region runs rewritten.
    let mut dropped = inlining::PcMap::new(header, caller.len());
    let mut setup = inlining::PcMap::new(header, caller.len());
    let mut rewritten = inlining::PcMap::new(header, caller.len());
    let caller_stored = inlining::stored_names(caller, sites);
    let mut checked_reads = inlining::PcMap::new(header, caller.len());
    let captured = receivers::captured(caller, mask(header)?);
    for (offset, instruction) in caller.iter().copied().enumerate() {
        if !quench_runtime::staged::calls(instruction.op()) {
            continue;
        }
        let pc = header + offset as u32;
        (instruction.result_flags() == 0 && !instruction.direct_eval() && !instruction.parameter_eval()).then_some(())?;
        let argc = ImmediateLayout::argument_count(instruction.imm());
        let window = ImmediateLayout::call_window_base(instruction.imm());
        let (function_register, mut receiver) = (instruction.register_b(), instruction.register_c());
        if !defines(function_register) {
            // A callee the loop never writes: the closure the register holds at entry, which the VM
            // checks at every native entry. Leaving in the call's setup, the straight run of pure
            // moves before it, resumes at the run's first instruction.
            (function_register < caller_registers
                && quench_runtime::compile::liveness::register_in(mask(header)?, usize::from(function_register))).then_some(())?;
            let start = (0..offset).rev().take_while(|index| pure_setup(caller[*index].op())).last().unwrap_or(offset);
            let resume = header + start as u32;
            let run = &caller[start..offset];
            setup_resumes(run, (resume, pc), mask(resume)?, &targets).then_some(())?;
            let (callee, reached) = inlining::admitted_callee(resolve, None, CalleeSource::Register(function_register), argc, sites)?;
            let bound = inlining::bound_parameters(run, resume, window, &callee, &reached, &defines, &mut dropped)?;
            for setup_pc in resume..pc {
                setup.insert(setup_pc, resume);
            }
            let base = Register::try_from(total).ok()?;
            total += u32::from(callee.registers);
            (total <= RegisterSet::BITS).then_some(())?;
            calls.insert(pc, inlining::Site {
                frame: InlineFrame {
                    call: pc, callee: callee.callee, function: callee.function, callee_register: function_register,
                    window, arguments: argc, result: instruction.result_register(),
                    guard: InlineGuard::Register(function_register), receiver, base, registers: callee.registers, bound,
                },
                callee_path: None, resume, window, result: instruction.result_register(), reached, callee,
            });
            continue;
        }
        // The callee's read: the last write of its register before the call, in a straight run.
        let read_offset = (0..offset).rev().find(|index| caller[*index].defined_register() == Some(function_register))?;
        let read = header + read_offset as u32;
        let definition = caller[read_offset];
        let capture = (is_load_capture_row(definition)
            && definition.capture_depth() == 0 && definition.result_flags() == 0)
            .then(|| definition.capture_slot());
        let mut callee_path = if capture.is_some() { None } else {
            Some(field_path(definition, sites, &std::collections::BTreeMap::new())?)
        };
        // A receiver the region writes is the call's own: a field read right before the
        // callee's, which reads the callee from it (`this.m.am(…)`), and nothing else
        // reads until the call, nor after it. The region gives that read a register of its
        // own, holding the field's value (`held_registers`), and the setup resumes at it.
        let mut resume_offset = read_offset;
        let checked = defines(receiver) && receivers::stored(caller, read_offset, receiver, sites, &|atom| caller_stored.contains(atom as usize));
        if checked {
            checked_reads.insert(pc, read);
            // The callee resolves now through the receiver's own path; native code checks
            // the read each iteration, so the path is never an entry snapshot.
            let own = read_offset - 1;
            let receiver_path = field_path(caller[own], sites, &std::collections::BTreeMap::new())?;
            let callee_path = callee_path.as_mut()?;
            (callee_path.root == FieldReceiver::Register(receiver)).then_some(())?;
            callee_path.root = receiver_path.root;
            callee_path.steps.splice(0..0, receiver_path.steps);
        } else if let Some((root, held)) = receivers::rerooted(caller, read_offset, receiver, &defines, callee_path.as_ref(), &captured) {
            (callee_path.as_mut()?.root, receiver) = (root, held);
        } else if defines(receiver) {
            let callee_path = callee_path.as_mut()?;
            let own = read_offset.checked_sub(1)?;
            let receiver_path = field_path(caller[own], sites, &std::collections::BTreeMap::new())?;
            (caller[own].defined_register() == Some(receiver)
                && receiver_path.root != FieldReceiver::Register(receiver)
                && callee_path.root == FieldReceiver::Register(receiver)
                && callee_path.steps.len() == 1
                && caller[read_offset + 1..offset].iter().all(|instruction| !mentions(*instruction, receiver, sites))
                && !quench_runtime::compile::liveness::register_in(mask(pc + 1)?, usize::from(receiver)))
            .then_some(())?;
            let fresh = Register::try_from(total).ok()?;
            total += 1;
            rewritten.insert(header + own as u32, super::super::meaning_compiled::field_read_record(fresh, caller[own].b(), caller[own].c(), caller[own].imm()));
            callee_path.root = receiver_path.root;
            callee_path.steps.splice(0..0, receiver_path.steps);
            receiver = fresh;
            resume_offset = own;
        }
        let resume = header + resume_offset as u32;
        let at_resume = mask(resume)?;
        let run = &caller[resume_offset + 1..offset];
        setup_resumes(run, (resume, pc), at_resume, &targets).then_some(())?;
        let source = capture.map_or(CalleeSource::Path, CalleeSource::Capture);
        let (callee, reached) = inlining::admitted_callee(resolve, callee_path.as_ref(), source, argc, sites)?;
        let first = read + 1;
        let bound = inlining::bound_parameters(&caller[read_offset + 1..offset], first, window, &callee, &reached, &defines, &mut dropped)?;
        if !checked {
            dropped.insert(read, ());
        }
        for setup_pc in resume..pc {
            setup.insert(setup_pc, resume);
        }
        let base = Register::try_from(total).ok()?;
        total += u32::from(callee.registers);
        (total <= RegisterSet::BITS).then_some(())?;
        calls.insert(
            pc,
            inlining::Site {
                frame: InlineFrame {
                    call: pc,
                    callee: callee.callee,
                    function: callee.function,
                    callee_register: function_register,
                    window,
                    arguments: argc,
                    result: instruction.result_register(),
                    guard: capture.map_or(InlineGuard::Field(usize::MAX), InlineGuard::Capture),
                    receiver,
                    base,
                    registers: callee.registers,
                    bound,
                },
                callee_path: callee_path.clone(),
                resume,
                window,
                result: instruction.result_register(),
                reached,
                callee,
            },
        );
    }
    (!calls.is_empty()).then_some(())?;
    let layout = inlining::layout(caller, header, &dropped, &calls, sites);
    (layout.length as usize <= MAX_REGION_INSTRUCTIONS).then_some(())?;
    let mut emission = inlining::Emission::new(inlining::Plan {
        header,
        backedge,
        caller,
        sites,
        live,
        calls: &calls,
        dropped: &dropped,
        setup: &setup,
        rewritten: &rewritten,
        layout: &layout,
    });
    emission.run()?;
    let inlining::Emission { instructions, origins, masks, callee_captures, .. } = emission;
    let receivers = object_registers(&calls, &checked_reads, caller, header);
    let header_live = masks.first().copied();
    let mut region = admit_region(
        instructions,
        total as usize,
        constants,
        sites,
        Some(std::rc::Rc::new(quench_runtime::compile::liveness::RegisterMasks::new(masks))),
        header_live,
        header,
        &receivers, MethodCalls::Inliner, &|_| false,
    )
    .ok()?;
    // Dynamic receiver reads use the loop's caller registers and exact caller PCs. Keep
    // inlined regions on the established entry-snapshot contract until their multi-frame
    // recovery maps can represent per-iteration cache misses.
    if !region.dynamic_field_reads.is_empty() {
        return None;
    }
    // Each callee's field, which the entry reads and guards still holds it, read through
    // the region's held registers.
    let held: std::collections::BTreeMap<Register, FieldPath> =
        region.held.iter().map(|(register, (path, _))| (*register, path.clone())).chain(captured).collect();
    let frames = inlining::guarded_frames(calls, &checked_reads, &|register| held.get(&register), caller_registers, &defines, &mut region)?;
    region.inlining = Some(Box::new(Inlining { origins, frames, callee_captures, caller_registers, latch: backedge }));
    Some(region)
}
