fn execute_generator_step(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: crate::completion::Completion,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let _private_environment = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let mut result = None;
    let input = completion.clone();
    generator.machine.borrow_mut().step(input, |_| {
        let step = crate::vm::execute_generator_step(
            generator.function.ops(),
            &mut state.registers,
            state.environment.clone(),
            state.pc,
            completion,
        )?;
        let completion = step.completion.clone();
        result = Some(step);
        Ok(completion)
    })?;
    result.ok_or(VmError::MissingReturn)
}
