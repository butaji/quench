//! Shallow composition of one counted pass loop with one ordered recurrence.

use super::iteration_slots_are_private;
use super::selection::{select, select_init, select_test, select_update, Bound, OrderedRecurrence};
use crate::{ir::Opcode, machine::CodeView};

pub(super) struct RepeatedRecurrence {
    pub(super) pass_slot: u16,
    pub(super) pass_start: i32,
    pub(super) pass_bound: Bound,
    pub(super) pass_inclusive: bool,
    pub(super) inner_dst: u16,
    pub(super) body_dst: u16,
    pub(super) recurrence: OrderedRecurrence,
}

pub(super) fn select_repeated(
    init: CodeView<'_>,
    test: CodeView<'_>,
    body: CodeView<'_>,
    update: CodeView<'_>,
) -> Option<RepeatedRecurrence> {
    let (pass_slot, pass_start) = select_init(init)?;
    let (pass_bound, pass_inclusive) = select_test(test, pass_slot)?;
    select_update(update, pass_slot)?;
    let (inner_dst, body_dst, recurrence) = select_nested_recurrence(body)?;
    Some(RepeatedRecurrence {
        pass_slot,
        pass_start,
        pass_bound,
        pass_inclusive,
        inner_dst,
        body_dst,
        recurrence,
    })
}

fn select_nested_recurrence(body: CodeView<'_>) -> Option<(u16, u16, OrderedRecurrence)> {
    let mut nested = None;
    let mut forwarded = None;
    for pc in 0..body.len() {
        let instruction = body.instruction(pc)?;
        match instruction.opcode {
            Opcode::Slow => select_nested_loop(body, pc, &mut nested)?,
            Opcode::LoadConst => select_undefined(body, instruction.b)?,
            Opcode::Move => select_forward(instruction, &nested, &mut forwarded)?,
            Opcode::Return => {}
            _ => return None,
        }
    }
    let (inner_dst, recurrence) = nested?;
    Some((inner_dst, forwarded.unwrap_or(inner_dst), recurrence))
}

fn select_nested_loop(
    body: CodeView<'_>,
    pc: usize,
    nested: &mut Option<(u16, OrderedRecurrence)>,
) -> Option<()> {
    let crate::ops::Op::Loop {
        init,
        test,
        body,
        update,
        post_test: false,
        dst,
        label: None,
        per_iteration,
    } = body.cold_at(pc)?
    else {
        return None;
    };
    let recurrence = select(init.code()?, test.code()?, body.code()?, update.code()?)?;
    iteration_slots_are_private(per_iteration, recurrence.index_slot)?;
    nested.replace((*dst, recurrence)).is_none().then_some(())
}

fn select_undefined(body: CodeView<'_>, constant: u16) -> Option<()> {
    matches!(body.constant(constant)?, crate::ops::Constant::Undefined).then_some(())
}

fn select_forward(
    instruction: crate::ir::Instruction,
    nested: &Option<(u16, OrderedRecurrence)>,
    forwarded: &mut Option<u16>,
) -> Option<()> {
    let (inner_dst, _) = nested.as_ref()?;
    (*inner_dst == instruction.b).then_some(())?;
    forwarded.replace(instruction.a).is_none().then_some(())
}
