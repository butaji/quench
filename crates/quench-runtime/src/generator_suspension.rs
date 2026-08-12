fn is_suspended(generator: &GeneratorData, state: &GeneratorState) -> bool {
    state.suspension.is_some() || suspended_context(generator, state).is_some()
}

#[derive(Clone, Copy)]
enum SuspendedContext {
    Yield,
    YieldStar,
    Try,
    Conditional,
    PrivateScope,
    IteratorBinding,
}

fn suspended_context(
    generator: &GeneratorData,
    state: &GeneratorState,
) -> Option<SuspendedContext> {
    if matches!(
        generator.function.ops().get(state.pc.wrapping_sub(1)),
        Some(Op::Yield { .. })
    ) {
        return Some(SuspendedContext::Yield);
    }
    if let Some(Op::YieldStar { iterator, .. }) = generator.function.ops().get(state.pc) {
        if crate::execute::read_register(&state.registers, *iterator)
            .is_ok_and(|value| !matches!(value, Value::Undefined))
        {
            return Some(SuspendedContext::YieldStar);
        }
    }
    if suspended_try(generator, state).is_some() {
        return Some(SuspendedContext::Try);
    }
    if suspended_conditional(generator, state).is_some() {
        return Some(SuspendedContext::Conditional);
    }
    if suspended_iterator_binding(generator, state).is_some()
        || suspended_iterator_conditional(generator, state).is_some()
    {
        return Some(SuspendedContext::IteratorBinding);
    }
    suspended_private_scope(generator, state).map(|_| SuspendedContext::PrivateScope)
}
