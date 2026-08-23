struct BranchFrameResume {
    branch_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    dst: u16,
    yield_dst: u16,
}

fn branch_frame_resume(generator: &GeneratorData) -> Option<BranchFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Branch { branch_resume, resume, dst, yield_dst, .. } = frame else {
        return None;
    };
    Some(BranchFrameResume { branch_resume, resume, dst, yield_dst })
}

fn resume_branch_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = branch_frame_resume(generator) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        generator.machine.borrow_mut().pop_frame();
        return Ok(Some(resume));
    }
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(frame.branch_resume).ok_or(VmError::MissingReturn)?;
    let step = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_step_in_place(ops, registers)
    })?;
    let completion = step.completion;
    if completion.is_suspension() {
        advance_frame_after_yield(generator, frame.branch_resume, step.next)?;
        return Ok(Some(completion));
    }
    let crate::completion::Completion::Return(value) = completion else {
        return Ok(Some(completion));
    };
    crate::execute::write_value(&mut registers_mut(generator), frame.dst, value);
    generator.machine.borrow_mut().pop_frame();
    resume_generator_range(generator, state, frame.resume, crate::completion::Completion::Normal).map(Some)
}

fn install_branch_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let Some(frame) = branch_frame_resume(generator) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), frame.yield_dst, input.clone());
    true
}
