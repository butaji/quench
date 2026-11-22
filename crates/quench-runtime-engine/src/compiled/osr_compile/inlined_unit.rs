//! The inlined unit of an inlining loop region: its frames (`unit_frames`), the call markers its inlined bodies
//! begin with (`insert_markers`, `marker_sites`), its jump targets and static field reads in unit PCs
//! (`localized_jumps`, `inlined_entry_fields`), and the unit they make up (`inlining_unit`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::Op;

/// The lane unit's frames for an inlining region's inlined callees.
pub(crate) fn unit_frames(region: &LoopRegion, inlining: &osr_loop::Inlining) -> Vec<lane_unit::InlinedCall> {
    inlining.frames.iter().map(|frame| lane_unit::InlinedCall {
        recovery: lane_unit::CallRecovery::Materialize,
        parent: None,
        call: frame.call,
        target: lane_unit::CallTarget {
            callee: lane_unit::CalleeBinding::Identity(frame.callee),
            function: frame.function,
        },
        callee_register: frame.callee_register,
        callee_entry_word: match frame.guard {
            osr_loop::InlineGuard::Field(index) => u32::try_from(osr_loop::field_word(region, index)).ok(),
            _ => None,
        },
        receiver: frame.receiver,
        window: frame.window,
        arguments: frame.arguments,
        result: Some(frame.result),
        base: frame.base,
        registers: frame.registers,
        alternative: None,
        function_call: None,
    }).collect()
}

/// Where each inlined frame's call marker goes: its first instruction's position and the PC
/// to resume at, where a failed identity guard must rerun the callee's read too.
pub(crate) fn marker_sites(origins: &[lane_unit::Origin], frames: &[lane_unit::InlinedCall]) -> Vec<(usize, u32)> {
    (0..frames.len()).filter_map(|frame| {
        let at = origins.iter().position(|origin|
            matches!(origin, lane_unit::Origin::Inlined { frame: at, .. } if *at == frame))?;
        // A failed identity guard must rerun the callee's read too: the region resolved it
        // at compile time and dropped it, so resume where its setup begins.
        let call = match at.checked_sub(1).and_then(|before| origins.get(before)).copied() {
            Some(lane_unit::Origin::Caller(resume)) => resume,
            _ => frames[frame].call,
        };
        Some((at, call))
    }).collect()
}

/// The region's instructions with each jump target a region-local PC (one past the end where it leaves).
pub(crate) fn localized_jumps(region: &LoopRegion) -> Vec<WideInstruction> {
    let count = region.instructions().len();
    let header = region.header as usize;
    let mut instructions = region.instructions().to_vec();
    for instruction in &mut instructions {
        if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget {
            let target = instruction.imm() as usize;
            let local = target.checked_sub(header).filter(|local| *local < count).unwrap_or(count);
            instruction.set_jump_target(local as u32);
        }
    }
    instructions
}

/// The region's static field reads, by the unit PC each instruction took, answered from
/// the entry record: an instruction at region offset `o` moved past every marker at or
/// before it (`inserted`).
pub(crate) fn inlined_entry_fields(region: &LoopRegion, program: &ResidualProgram, inserted: &[(usize, u32)]) -> Vec<Option<u32>> {
    let unit_pc = |offset: usize| offset + inserted.iter().filter(|(at, _)| *at <= offset).count();
    super::super::lane_region::flat_table(region.field_reads().iter()
        .filter(|(_, index)| entry_read(region, **index, program))
        .filter_map(|(offset, index)| Some((unit_pc(*offset as usize),
            u32::try_from(osr_loop::field_word(region, *index)).ok()?))))
}

/// Inserts each frame's call marker: a `Nop` where its body begins, at which the lane binds the
/// callee and passes the call's arguments, as `lane_unit::unit` emits it. The region's own
/// parameter moves precede the marker and write the same registers. Returns the insertions
/// (position, resume PC) and the markers' unit PCs.
pub(crate) fn insert_markers(instructions: &mut Vec<WideInstruction>, origins: &mut Vec<lane_unit::Origin>,
    frames: &[lane_unit::InlinedCall]) -> (Vec<(usize, u32)>, Vec<usize>) {
    let mut inserted = marker_sites(origins, frames);
    inserted.sort_unstable_by_key(|(at, _)| *at);
    inserted.dedup_by_key(|(at, _)| *at);
    let mut markers: Vec<usize> = inserted.iter().map(|(at, _)| *at).collect();
    // The frame each instruction runs in, before the markers move them.
    let running: Vec<Option<usize>> = origins.iter().map(|origin| match origin {
        lane_unit::Origin::Inlined { frame, .. } => Some(*frame),
        _ => None,
    }).collect();
    for &(at, call) in inserted.iter().rev() {
        instructions.insert(at, WideInstruction::new(Op::Nop, 0, 0, 0, 0));
        origins.insert(at, lane_unit::Origin::Caller(call));
    }
    // A jump target lands on the marker at its own position, as the call's PC did: an
    // insertion at the target itself is that marker, so only earlier ones move it. A callee's
    // own jump to its first instruction (a loop at its entry) is no call: it lands past its
    // frame's marker, which would bind the call's arguments to the parameters again.
    let within = |frame: usize, mut inner: usize| loop {
        if inner == frame { return true; }
        match frames.get(inner).and_then(|candidate| candidate.parent) { Some(parent) => inner = parent, None => return false }
    };
    let shift = |target: usize, from: Option<usize>| target + inserted.iter().filter(|(at, _)| *at < target
        || (*at == target && from.is_some_and(|inner| running.get(target).copied().flatten().is_some_and(|frame| within(frame, inner))))).count();
    let mut original = 0;
    for (unit_pc, instruction) in instructions.iter_mut().enumerate() {
        if inserted.iter().any(|(at, _)| *at + inserted.iter().filter(|(before, _)| before < at).count() == unit_pc) { continue; }
        let from = running.get(original).copied().flatten();
        original += 1;
        if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget {
            instruction.set_jump_target(shift(instruction.imm() as usize, from) as u32);
        }
    }
    for marker in &mut markers { *marker = shift(*marker, None); }
    (inserted, markers)
}

/// Map an inlining loop region onto the lane unit: its instructions' jump targets become
/// unit PCs, its origins name the caller/inlined frame of each instruction, and its frames
/// carry the callee identity, function, call window and result.
pub(crate) fn inlining_unit(region: &LoopRegion, program: &ResidualProgram) -> Option<(lane_unit::Unit, Vec<Option<u32>>)> {
    let inlining = region.inlining.as_deref()?;
    let count = region.instructions().len();
    let mut instructions = localized_jumps(region);
    // A callee's own capture read names its slot in the callee's environment again: the unit's
    // frame reads it from that environment's dense slots (`Observation::frame_captures`), as a
    // lane body's inlined frame does, not from the entry's snapshot of it.
    for (instruction, origin) in instructions.iter_mut().zip(&inlining.origins) {
        if matches!(origin, osr_loop::Origin::Inlined { .. })
            && let Some(quench_runtime::staged::Operation::<(), ()>::LoadCapture { depth: 0, slot: own, .. }) = quench_runtime::staged::Operation::decode(*instruction, quench_runtime::staged::DecodeContext::EMPTY)
            && own & osr_loop::CALLEE_CAPTURE != 0
            && let Some(&(_, slot)) = inlining.callee_captures.get(usize::from(own & !osr_loop::CALLEE_CAPTURE)) {
            *instruction = WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(),
                quench_runtime::bytecode::ImmediateLayout::capture_immediate(0, slot));
        }
    }
    let mut origins: Vec<lane_unit::Origin> = inlining.origins.iter().take(count).map(|origin| match origin {
        osr_loop::Origin::Caller(pc) => lane_unit::Origin::Caller(*pc),
        osr_loop::Origin::Inlined { frame, pc } => lane_unit::Origin::Inlined { frame: usize::from(*frame), pc: *pc },
        osr_loop::Origin::Leave(pc) => lane_unit::Origin::Caller(*pc),
    }).collect();
    let frames = unit_frames(region, inlining);
    let (inserted, markers) = insert_markers(&mut instructions, &mut origins, &frames);
    let registers = frames.iter().map(|frame| frame.base + frame.registers)
        .max().unwrap_or(0).max(inlining.caller_registers);
    let entry_fields = inlined_entry_fields(region, program, &inserted);
    Some((lane_unit::Unit {
        instructions, origins, frames, markers,
        caller_registers: inlining.caller_registers, registers,
        // The caller PC past the loop, where an edge out of it resumes.
        end: inlining.latch + 1,
    }, entry_fields))
}
