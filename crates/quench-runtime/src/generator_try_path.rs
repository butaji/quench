struct InitialTryFrame {
    phase: crate::machine::TryPhase,
    body: crate::machine::CodeRange,
    handler: Option<crate::machine::CodeRange>,
    finalizer: Option<crate::machine::CodeRange>,
    body_resume: crate::machine::CodeRange,
    yield_dst: u16,
    catch_slot: Option<u16>,
}

struct InitialTryPath {
    frames: Vec<InitialTryFrame>,
    yield_dst: u16,
    continuation: crate::machine::CodeRange,
}

fn push_try_frames_for_point(
    generator: &GeneratorData,
    point: &crate::continuation::SuspensionPoint,
) -> Result<bool, VmError> {
    let Some(target) = suspension_child_resume(point) else {
        return Ok(false);
    };
    let Some(code) = generator.function.code.code() else {
        return Ok(false);
    };
    let yield_dst = suspension_destination(point).ok_or(VmError::MissingReturn)?;
    let Some(path) =
        initial_try_path_to_range(code, generator.function.code.range, target, yield_dst)
    else {
        return Ok(false);
    };
    push_initial_try_path(generator, path)
}

fn push_initial_try_path(generator: &GeneratorData, path: InitialTryPath) -> Result<bool, VmError> {
    let has_frames = !path.frames.is_empty();
    let outer_resume = parent_resume_range(generator, &empty_generator_state());
    let empty = empty_range_at_end(outer_resume);
    for (index, frame) in path.frames.into_iter().enumerate() {
        let resume = if index == 0 { outer_resume } else { empty };
        try_push_frame(
            &mut generator.machine.borrow_mut(),
            frame.into_machine(resume),
        )?;
    }
    Ok(has_frames)
}

impl InitialTryFrame {
    fn into_machine(self, resume: crate::machine::CodeRange) -> crate::machine::Frame {
        crate::machine::Frame::Try {
            phase: self.phase,
            body: self.body,
            handler: self.handler,
            finalizer: self.finalizer,
            body_resume: self.body_resume,
            resume,
            yield_dst: self.yield_dst,
            catch_slot: self.catch_slot,
        }
    }
}

fn empty_generator_state() -> GeneratorState {
    GeneratorState {
        nested: 0,
        private_environment: None,
        suspension: None,
        async_for_of: None,
        pending_completion: None,
    }
}

fn empty_range_at_end(range: crate::machine::CodeRange) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.end,
        end: range.end,
    }
}

fn initial_try_path_to_range(
    code: crate::machine::CodeView<'_>,
    range: crate::machine::CodeRange,
    target: crate::machine::CodeRange,
    yield_dst: u16,
) -> Option<InitialTryPath> {
    if range_contains(range, target) {
        return Some(InitialTryPath {
            frames: Vec::new(),
            yield_dst,
            continuation: target,
        });
    }
    code.cold_ops().find_map(|(index, op)| {
        try_path_through_op(op, target, yield_dst)
            .map(|path| complete_parent_path(range, index, path))
    })
}

fn try_path_through_op(
    op: &Op,
    target: crate::machine::CodeRange,
    yield_dst: u16,
) -> Option<InitialTryPath> {
    if let Op::Try { .. } = op {
        return try_path_through_try(op, target, yield_dst);
    }
    let mut found = None;
    op.visit_frame_fragments(&mut |child| {
        if found.is_none() {
            found = path_in_child(child, target, yield_dst);
        }
    });
    found
}

fn try_path_through_try(
    op: &Op,
    target: crate::machine::CodeRange,
    yield_dst: u16,
) -> Option<InitialTryPath> {
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        ..
    } = op
    else {
        return None;
    };
    for (branch, phase) in try_branches(body, handler.as_ref(), finalizer.as_ref()) {
        let Some(mut path) = path_in_child(branch, target, yield_dst) else {
            continue;
        };
        path.frames.insert(
            0,
            try_frame(op, phase, path.continuation, yield_dst, *catch_slot),
        );
        return Some(path);
    }
    None
}

fn try_frame(
    op: &Op,
    phase: crate::machine::TryPhase,
    body_resume: crate::machine::CodeRange,
    yield_dst: u16,
    catch_slot: Option<u16>,
) -> InitialTryFrame {
    let Op::Try {
        body,
        handler,
        finalizer,
        ..
    } = op
    else {
        unreachable!()
    };
    InitialTryFrame {
        phase,
        body: body.range,
        handler: handler.as_ref().map(|code| code.range),
        finalizer: finalizer.as_ref().map(|code| code.range),
        body_resume,
        yield_dst,
        catch_slot,
    }
}

fn try_branches<'a>(
    body: &'a crate::machine::FunctionCode,
    handler: Option<&'a crate::machine::FunctionCode>,
    finalizer: Option<&'a crate::machine::FunctionCode>,
) -> impl Iterator<Item = (&'a crate::machine::FunctionCode, crate::machine::TryPhase)> {
    [
        (Some(body), crate::machine::TryPhase::Body),
        (handler, crate::machine::TryPhase::Catch),
        (finalizer, crate::machine::TryPhase::Finally),
    ]
    .into_iter()
    .filter_map(|(code, phase)| code.map(|code| (code, phase)))
}

fn path_in_child(
    child: &crate::machine::FunctionCode,
    target: crate::machine::CodeRange,
    yield_dst: u16,
) -> Option<InitialTryPath> {
    initial_try_path_to_range(child.code()?, child.range, target, yield_dst)
}

fn complete_parent_path(
    range: crate::machine::CodeRange,
    index: usize,
    mut path: InitialTryPath,
) -> InitialTryPath {
    path.continuation = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(index as u32 + 1),
        end: range.end,
    };
    path
}

fn range_contains(outer: crate::machine::CodeRange, inner: crate::machine::CodeRange) -> bool {
    outer.code == inner.code && inner.start >= outer.start && inner.end <= outer.end
}
