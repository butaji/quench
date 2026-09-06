fn install_resume_input(generator: &GeneratorData, state: &mut GeneratorState, input: Value) {
    if install_direct_resume_input(generator, state, &input) {
        return;
    }
    if install_frame_resume_input(generator, state, &input) {
        return;
    }
    if let Some((_, Op::Yield { src }, _)) = suspended_try(generator, state) {
        crate::execute::write_value(&mut registers_mut(generator), *src, input);
        return;
    }
    install_scanned_resume_input(generator, state, input);
}

fn install_direct_resume_input(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: &Value,
) -> bool {
    let Some(point) = state.suspension.take() else {
        return false;
    };
    if matches!(
        point,
        crate::continuation::SuspensionPoint::YieldStar { .. }
    ) {
        generator.machine.borrow_mut().pop_frame();
    }
    let src = match point {
        crate::continuation::SuspensionPoint::Yield { src, .. }
        | crate::continuation::SuspensionPoint::YieldStar { dst: src, .. }
        | crate::continuation::SuspensionPoint::Loop { yield_dst: src, .. }
        | crate::continuation::SuspensionPoint::Branch { yield_dst: src, .. } => src,
        crate::continuation::SuspensionPoint::Nested { inner, .. } => {
            return install_nested_point_input(generator, &inner, input);
        }
    };
    crate::execute::write_value(&mut registers_mut(generator), src, input.clone());
    true
}

fn install_nested_point_input(
    generator: &GeneratorData,
    point: &crate::continuation::SuspensionPoint,
    input: &Value,
) -> bool {
    let slot = match point {
        crate::continuation::SuspensionPoint::Yield { src, .. }
        | crate::continuation::SuspensionPoint::YieldStar { dst: src, .. }
        | crate::continuation::SuspensionPoint::Loop { yield_dst: src, .. }
        | crate::continuation::SuspensionPoint::Branch { yield_dst: src, .. } => *src,
        crate::continuation::SuspensionPoint::Nested { inner, .. } => {
            return install_nested_point_input(generator, inner, input);
        }
    };
    crate::execute::write_value(&mut registers_mut(generator), slot, input.clone());
    true
}

fn install_frame_resume_input(
    generator: &GeneratorData,
    _state: &mut GeneratorState,
    input: &Value,
) -> bool {
    if let Some(crate::machine::Frame::Await { destination, .. }) =
        generator.machine.borrow().frames.frames.last()
    {
        crate::execute::write_value(&mut registers_mut(generator), *destination, input.clone());
        return true;
    }
    let code = generator.function.code.code();
    if let Some(Op::YieldStar { dst, .. }) =
        code.and_then(|code| code.cold_at(machine_pc(generator)))
    {
        crate::execute::write_value(&mut registers_mut(generator), *dst, input.clone());
        return true;
    }
    install_delegate_frame_input(generator, input)
        || install_dispose_frame_input(generator, input)
        || install_try_frame_input(generator, input)
        || install_iterator_frame_input(generator, input)
        || install_private_frame_input(generator, input)
        || install_loop_frame_input(generator, input)
        || install_branch_frame_input(generator, input)
}

fn install_scanned_resume_input(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: Value,
) {
    let Some(index) = machine_pc(generator).checked_sub(1) else {
        return;
    };
    let Some(Op::Yield { src }) = generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(index))
    else {
        install_nested_resume_input(generator, state, input);
        return;
    };
    crate::execute::write_value(&mut registers_mut(generator), *src, input);
}
