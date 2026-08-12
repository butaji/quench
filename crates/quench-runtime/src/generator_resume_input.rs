fn install_resume_input(generator: &GeneratorData, state: &mut GeneratorState, input: Value) {
    if install_direct_resume_input(generator, state, &input) { return; }
    if install_frame_resume_input(generator, state, &input) { return; }
    if let Some((_, Op::Yield { src }, _)) = suspended_try(generator, state) {
        crate::execute::write_value(&mut state.registers, *src, input);
        return;
    }
    install_scanned_resume_input(generator, state, input);
}

fn install_direct_resume_input(generator: &GeneratorData, state: &mut GeneratorState, input: &Value) -> bool {
    let Some(point) = state.suspension.take() else { return false; };
    if matches!(point, crate::continuation::SuspensionPoint::YieldStar { .. }) { generator.machine.borrow_mut().pop_frame(); }
    let src = match point {
        crate::continuation::SuspensionPoint::Yield { src, .. }
        | crate::continuation::SuspensionPoint::YieldStar { dst: src, .. } => src,
    };
    crate::execute::write_value(&mut state.registers, src, input.clone());
    true
}

fn install_frame_resume_input(generator: &GeneratorData, state: &mut GeneratorState, input: &Value) -> bool {
    if let Some(Op::YieldStar { dst, .. }) = generator.function.ops().get(state.pc) {
        crate::execute::write_value(&mut state.registers, *dst, input.clone());
        return true;
    }
    install_try_frame_input(generator, &mut state.registers, input)
        || install_private_frame_input(generator, &mut state.registers, input)
        || install_branch_frame_input(generator, &mut state.registers, input)
}

fn install_scanned_resume_input(generator: &GeneratorData, state: &mut GeneratorState, input: Value) {
    let Some(index) = state.pc.checked_sub(1) else { return; };
    let Some(Op::Yield { src }) = generator.function.ops().get(index) else {
        install_nested_resume_input(generator, state, input);
        return;
    };
    crate::execute::write_value(&mut state.registers, *src, input);
}
