//! An image body's misses (`LaneHelpers::Ahead`): an image has no feedback and no later tier, so a
//! row whose guard fails before its effects (a field read no site entry answers, a call its helper
//! refuses, a builtin construction, an element store, an instruction no row takes) makes the record
//! by the interpreter's own step in place (`LaneHelper::Step`), or by its opcode's generic semantic
//! row where the step helper does not make it (a guarded operator or comparison), and continues at
//! the next record or the record's jump target, rather than leaving the body. The step's chain is laid out cold behind the row's run.

use super::*;

/// Whether the step helper makes `instruction` in place and continues: a general step or a script
/// call, with no result flag, no jump, and no binding of `this`.
fn steps_and_continues(instruction: WideInstruction) -> bool {
    let op = instruction.op();
    instruction.result_flags() == 0 && !op.control_flow_layout().jumps() && !op.binds_this() && !op.suspends()
        && !instruction.writes_current_this()
        && (op.makes_script_call() || (op.steps_in_body() && instruction.written_registers().count() <= STEP_WRITES))
}

/// `run`, the row of `decoded` at `pc`, with every exit that leaves before its effects (`Exit::Spilling`
/// at `pc`, `Exit::At` its record) taken to the step helper's chain behind it, where an image body
/// makes the record that way (`steps_and_continues`); `run` as it is otherwise, or where nothing leaves.
pub(crate) fn stepped_misses(run: Planned, decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Planned, String> {
    if helpers != LaneHelpers::Ahead || decoded.completes || layout.inlined(pc) || run.stop.is_some() { return Ok(run); }
    let record = layout.record(pc);
    let Some(instruction) = layout.span.instruction(record) else { return Ok(run) };
    // A comparison jump whose row guards Number operands (`!=` of two objects) still leaves: stepping it in place keeps
    // the body's callers nested on the native stack, and a deep recursion (earley-boyer's rewriter) then leaves at its
    // callee's entry instead, each such leave making every nested activation a frame.
    let stepped = steps_and_continues(instruction);
    if !stepped && (!helpers.generic_rows() || instruction.returns_from_frame()) { return Ok(run); }
    let leaves = |exit: &Exit| matches!(*exit, Exit::Spilling(at) if at == pc) || matches!(*exit, Exit::At(at) if at == record);
    let parts = run.pre.len() + 1 + run.post.len();
    let missing = run.pre.iter().chain(std::iter::once(&run)).chain(&run.post).any(|part| part.exits.iter().any(leaves));
    if !missing || run.pre.iter().chain(&run.post).any(|part| part.stop.is_some()) { return Ok(run); }
    // The hot run ends in a jump to the next record, which the layout drops where that record follows;
    // the step's chain sits behind it, its first part at `slow`.
    let slow = parts + 1;
    let mut chain = if stepped {
        super::helper_chain(layout, pc, &layout.spills(pc + 1), (LaneHelper::Step, helpers), None)?
    } else {
        super::generic_row(layout, decoded, pc, super::Semantic::of(layout, pc)?)?
    };
    let mut run = run;
    let retarget = |part: &mut Planned, index: usize| {
        for exit in &mut part.exits {
            if leaves(exit) { *exit = Exit::Ahead(slow - index); }
        }
    };
    let pre_len = run.pre.len();
    for (index, part) in run.pre.iter_mut().enumerate() { retarget(part, index); }
    retarget(&mut run, pre_len);
    for (index, part) in run.post.iter_mut().enumerate() { retarget(part, pre_len + 1 + index); }
    let next = Planned::single(super::control(Kind::Jump)?, [0; HOLES], Some(pc + 1), None);
    let (lead, tail) = (std::mem::take(&mut chain.pre), std::mem::take(&mut chain.post));
    let cold = lead.into_iter().chain(std::iter::once(chain)).chain(tail).map(|mut part| {
        part.branch = part.branch.map(|branch| match branch {
            Branch::Part(index) => Branch::Part(slow + index),
            pc @ Branch::Pc(_) => pc,
        });
        part.cold = true;
        part
    });
    run.post.push(next);
    run.post.extend(cold);
    Ok(run)
}
