fn collect_for_of_frames(
    op: &Op,
    resume: crate::machine::CodeRange,
    registers: &crate::register_file::RegisterFile,
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Op::ForOf { slot, body, .. } = op else {
        return Ok(false);
    };
    let Some(loop_iterator) = crate::loops::take_live_for_of() else {
        return Ok(false);
    };
    let Some(ops) = body.code() else {
        return Err(VmError::MissingReturn);
    };
    collect_loop_body_frames(ops, body.range, loop_iterator, *slot, resume, registers, frames)
}

fn collect_loop_body_frames(
    ops: crate::machine::CodeView<'_>,
    body: crate::machine::CodeRange,
    loop_iterator: Value,
    slot: u16,
    resume: crate::machine::CodeRange,
    registers: &crate::register_file::RegisterFile,
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    for (index, op) in ops.cold_ops() {
        if matches!(op, Op::Await { .. }) {
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                0,
                slot,
            ));
            return Ok(true);
        }
        if let Op::Yield { src } = op {
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                *src,
                slot,
            ));
            return Ok(true);
        }
        if let Op::YieldStar {
            dst,
            source,
            iterator,
        } = op
        {
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                0,
                slot,
            ));
            let iterator_value = crate::execute::read_register(registers, *iterator)?;
            let iterator = if matches!(iterator_value, Value::Undefined) {
                let source = crate::execute::read_register(registers, *source)?;
                crate::collections::iterator::delegate_start(source)?
            } else {
                iterator_value
            };
            frames.push(crate::machine::Frame::Delegate {
                phase: 0,
                iterator,
                destination: *dst,
            });
            return Ok(true);
        }
        if let Op::ForOf {
            slot: nested_slot,
            body: nested_body,
            ..
        } = op
        {
            let Some(nested_iterator) = crate::loops::take_live_for_of() else {
                continue;
            };
            let nested_resume = range_after_iterator_op(body, index);
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
                body,
                nested_resume,
                resume,
                0,
                slot,
            ));
            if collect_nested_for_of_frame(
                nested_iterator,
                nested_body,
                nested_resume,
                *nested_slot,
                registers,
                frames,
            )? {
                return Ok(true);
            }
            frames.pop();
        }
        if matches!(op, Op::IteratorBinding { .. }) {
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
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
        if conditional_contains_yield(op) {
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                0,
                slot,
            ));
            return Ok(true);
        }
        if try_contains_yield(op) {
            frames.push(for_of_repeat_frame(
                loop_iterator.clone(),
                body,
                range_after_iterator_op(body, index),
                resume,
                0,
                slot,
            ));
            if collect_try_frames(
                op,
                range_after_iterator_op(body, index),
                registers,
                frames,
            )? {
                return Ok(true);
            }
            frames.pop();
        }
    }
    Ok(false)
}

fn conditional_contains_yield(op: &Op) -> bool {
    let Op::Conditional {
        consequent,
        alternate,
        ..
    } = op
    else {
        return false;
    };
    consequent.code().is_some_and(ops_contain_yield)
        || alternate.code().is_some_and(ops_contain_yield)
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
    body.code().is_some_and(ops_contain_yield)
        || handler
            .as_ref()
            .and_then(|body| body.code())
            .is_some_and(ops_contain_yield)
        || finalizer
            .as_ref()
            .and_then(|body| body.code())
            .is_some_and(ops_contain_yield)
}

fn collect_try_frames(
    op: &Op,
    resume: crate::machine::CodeRange,
    registers: &crate::register_file::RegisterFile,
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        ..
    } = op
    else {
        return Ok(false);
    };

    let branches = [
        (crate::machine::TryPhase::Body, Some(body)),
        (crate::machine::TryPhase::Catch, handler.as_ref()),
        (crate::machine::TryPhase::Finally, finalizer.as_ref()),
    ];
    // The enclosing iterator frame owns the suffix after the try operation.
    // Resume the try into an empty range so that suffix is executed exactly
    // once by the iterator frame's own body continuation.
    let try_resume = crate::machine::CodeRange {
        code: resume.code,
        start: resume.start,
        end: resume.start,
    };
    for (phase, branch) in branches {
        let Some(branch) = branch else { continue };
        let Some(code) = branch.code() else {
            return Err(VmError::MissingReturn);
        };
        for (index, nested) in code.cold_ops() {
            if let Op::Yield { src } | Op::Await { dst: src, .. } = nested {
                frames.push(crate::machine::Frame::Try {
                    phase,
                    body: body.range,
                    handler: handler.as_ref().map(|body| body.range),
                    finalizer: finalizer.as_ref().map(|body| body.range),
                    body_resume: range_after_iterator_op(branch.range, index),
                    resume: try_resume,
                    yield_dst: *src,
                    catch_slot: *catch_slot,
                });
                return Ok(true);
            }
            if let Op::YieldStar { dst, iterator, .. } = nested {
                frames.push(crate::machine::Frame::Try {
                    phase,
                    body: body.range,
                    handler: handler.as_ref().map(|body| body.range),
                    finalizer: finalizer.as_ref().map(|body| body.range),
                    body_resume: range_after_iterator_op(branch.range, index),
                    resume: try_resume,
                    yield_dst: *dst,
                    catch_slot: *catch_slot,
                });
                let iterator = crate::execute::read_register(registers, *iterator)?;
                frames.push(crate::machine::Frame::Delegate {
                    phase: 0,
                    iterator,
                    destination: *dst,
                });
                return Ok(true);
            }
        }
    }
    Ok(false)
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

fn ops_contain_yield(ops: crate::machine::CodeView<'_>) -> bool {
    ops.cold_ops().any(|(_, op)| op_contains_yield(op))
}

fn collect_iterator_frames(
    binding: &Op,
    resume: crate::machine::CodeRange,
    registers: &crate::register_file::RegisterFile,
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
    let Some(ops) = body.code() else {
        return Err(VmError::MissingReturn);
    };
    for (index, op) in ops.cold_ops() {
        if matches!(op, Op::Await { .. }) {
            frames.push(iterator_frame(
                *iterator,
                body.range,
                resume,
                index,
                0,
                *close_normal,
                registers,
            )?);
            return Ok(true);
        }
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

fn collect_nested_for_of_frame(
    iterator: Value,
    body: &crate::machine::FunctionCode,
    resume: crate::machine::CodeRange,
    slot: u16,
    _registers: &crate::register_file::RegisterFile,
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Some(ops) = body.code() else {
        return Err(VmError::MissingReturn);
    };
    let Some((index, op)) = ops.cold_ops().find(|(_, op)| {
        matches!(op, Op::Yield { .. } | Op::Await { .. })
    }) else {
        return Ok(false);
    };
    let yield_dst = match op {
        Op::Yield { src } | Op::Await { dst: src, .. } => *src,
        _ => return Ok(false),
    };
    frames.push(for_of_repeat_frame(
        iterator,
        body.range,
        range_after_iterator_op(body.range, index),
        resume,
        yield_dst,
        slot,
    ));
    Ok(true)
}

fn collect_nested_yield_frame(
    code: &crate::machine::FunctionCode,
    binding: u16,
    body: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    close_normal: bool,
    registers: &crate::register_file::RegisterFile,
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Some(ops) = code.code() else {
        return Ok(false);
    };
    let Some((index, op)) = ops.find_cold(|op| matches!(op, Op::Yield { .. } | Op::Await { .. }))
    else {
        return Ok(false);
    };
    let src = match op {
        Op::Yield { src } | Op::Await { dst: src, .. } => *src,
        _ => return Ok(false),
    };
    let iterator = crate::execute::read_register(registers, binding)?;
    frames.push(iterator_binding_frame(
        iterator,
        binding,
        body,
        range_after_iterator_op(code.range, index),
        resume,
        (src, close_normal, false, 0),
    ));
    Ok(true)
}
