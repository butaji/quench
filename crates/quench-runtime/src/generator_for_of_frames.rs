fn collect_for_of_frames(
    op: &Op,
    resume: crate::machine::CodeRange,
    registers: &[Value],
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Op::ForOf { slot, body, .. } = op else {
        return Ok(false);
    };
    let Some(iterator) = crate::loops::live_for_of() else {
        return Ok(false);
    };
    let Some(ops) = body.ops() else {
        return Err(VmError::MissingReturn);
    };
    collect_loop_body_frames(ops, body.range, iterator, *slot, resume, registers, frames)
}

fn collect_loop_body_frames(
    ops: &[Op],
    body: crate::machine::CodeRange,
    iterator: Value,
    slot: u16,
    resume: crate::machine::CodeRange,
    registers: &[Value],
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    for (index, op) in ops.iter().enumerate() {
        if let Op::Yield { src } = op {
            frames.push(for_of_repeat_frame(
                iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                *src,
                slot,
            ));
            return Ok(true);
        }
        if matches!(op, Op::IteratorBinding { .. }) {
            frames.push(for_of_repeat_frame(
                iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                0,
                slot,
            ));
            if collect_iterator_frames(op, range_after_iterator_op(body, index), registers, frames)?
            {
                return Ok(true);
            }
            frames.pop();
            return Ok(false);
        }
        if try_contains_yield(op) {
            frames.push(for_of_repeat_frame(
                iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                0,
                slot,
            ));
            return Ok(true);
        }
    }
    Ok(false)
}

fn try_contains_yield(op: &Op) -> bool {
    let Op::Try {
        body,
        handler,
        finalizer,
        ..
    } = op
    else {
        return false;
    };
    body.ops().is_some_and(ops_contain_yield)
        || handler
            .as_ref()
            .and_then(|body| body.ops())
            .is_some_and(ops_contain_yield)
        || finalizer
            .as_ref()
            .and_then(|body| body.ops())
            .is_some_and(ops_contain_yield)
}

fn for_of_repeat_frame(
    iterator: Value,
    body: crate::machine::CodeRange,
    body_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
    slot: u16,
) -> crate::machine::Frame {
    iterator_binding_frame(
        iterator,
        0,
        body,
        body_resume,
        resume,
        (yield_dst, true, true, slot),
    )
}

fn ops_contain_yield(ops: &[Op]) -> bool {
    ops.iter().any(|op| matches!(op, Op::Yield { .. }))
}

fn collect_iterator_frames(
    binding: &Op,
    resume: crate::machine::CodeRange,
    registers: &[Value],
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Op::IteratorBinding {
        iterator,
        body,
        close_normal,
    } = binding
    else {
        return Ok(false);
    };
    let Some(ops) = body.ops() else {
        return Err(VmError::MissingReturn);
    };
    for (index, op) in ops.iter().enumerate() {
        if let Op::Yield { src } = op {
            frames.push(iterator_frame(
                *iterator,
                body.range,
                resume,
                index,
                *src,
                *close_normal,
                registers,
            )?);
            return Ok(true);
        }
        if let Op::Conditional {
            consequent,
            alternate,
            ..
        } = op
        {
            if collect_nested_yield_frame(
                consequent,
                *iterator,
                body.range,
                resume,
                *close_normal,
                registers,
                frames,
            )? || collect_nested_yield_frame(
                alternate,
                *iterator,
                body.range,
                resume,
                *close_normal,
                registers,
                frames,
            )? {
                return Ok(true);
            }
        }
        if matches!(op, Op::IteratorBinding { .. }) {
            let next = range_after_iterator_op(body.range, index);
            frames.push(iterator_frame(
                *iterator,
                body.range,
                resume,
                index,
                0,
                *close_normal,
                registers,
            )?);
            if collect_iterator_frames(op, next, registers, frames)? {
                return Ok(true);
            }
            frames.pop();
        }
    }
    Ok(false)
}

fn collect_nested_yield_frame(
    code: &crate::machine::FunctionCode,
    binding: u16,
    body: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    close_normal: bool,
    registers: &[Value],
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Some(ops) = code.ops() else {
        return Ok(false);
    };
    let Some((index, Op::Yield { src })) = ops
        .iter()
        .enumerate()
        .find(|(_, op)| matches!(op, Op::Yield { .. }))
    else {
        return Ok(false);
    };
    let iterator = crate::execute::read_register(registers, binding)?;
    frames.push(iterator_binding_frame(
        iterator,
        binding,
        body,
        range_after_iterator_op(code.range, index),
        resume,
        (*src, close_normal, false, 0),
    ));
    Ok(true)
}
