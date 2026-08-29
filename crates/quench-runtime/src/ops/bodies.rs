impl Op {
    pub(crate) fn capture_slots(&self, slots: &mut Vec<u16>) {
        use Op::*;
        match self {
            StoreLocal { slot, .. } | AliasLocal { slot, .. }
            | StoreFunctionName { slot, .. } | MarkUninitialized { slot, .. }
            | MarkImmutable { slot } | CheckInitialized { slot, .. }
            | InitializeLocal { slot } | LoadParameter { slot, .. }
            | DeclareEvalBinding { slot, .. } | DeclareGlobalLexicalBinding { slot, .. }
            | DeleteEvalBinding { slot, .. } | CreateGlobalFunction { slot, .. }
            | CreateGlobalVar { slot, .. } | InitializeResolvedBinding { slot, .. }
            | SetResolvedLocalBinding { slot, .. } | LoadResolvedLocalBinding { slot, .. }
            | LoadLocal { slot, .. } | ForIn { slot, .. }
            | ForOf { slot, .. } => slots.push(*slot),
            MakeRest { slot, arguments, .. } => {
                slots.push(*slot);
                slots.push(*arguments);
            }
            LoadBinding { slot, dynamic: false, .. } => slots.push(*slot),
            Try { catch_slot: Some(slot), .. } => slots.push(*slot),
            Loop { per_iteration, .. } => slots.extend_from_slice(per_iteration),
            CallSuperConstructor { .. }
            | CallSuperMethod { .. }
            | GetSuperProperty { .. }
            | GetSuperPropertyDynamic { .. }
            | SetSuperProperty { .. }
            | SetSuperPropertyDynamic { .. }
            | Eval { .. }
            | ResolveName { .. }
            | ResolveNameOrUndefined { .. }
            | SetName { .. } | DeleteName { .. } | LoadBinding { dynamic: true, .. } => {
                slots.push(u16::MAX)
            }
            _ => {}
        }
        self.visit_bodies(&mut |body| slots.extend_from_slice(body.capture_slots()));
    }

    pub(crate) fn rehome_bodies(
        &mut self,
        arena: &mut crate::machine::CodeArena,
        store: &std::rc::Rc<std::sync::OnceLock<std::rc::Rc<crate::machine::CodeStore>>>,
    ) {
        match self {
            Self::AppendInstanceField(op) => {
                if let Some(initializer) = &mut op.initializer {
                    initializer.body.rehome(arena, store);
                }
            }
            Self::MakeFunction { body, .. }
            | Self::MakeFunctionWithKind { body, .. }
            | Self::Label { body, .. }
            | Self::With { body, .. }
            | Self::WithDispose { body, .. }
            | Self::PrivateScope { body, .. }
            | Self::StaticBlock { body, .. }
            | Self::IteratorBinding { body, .. }
            | Self::ForIn { body, .. }
            | Self::ForOf { body, .. } => body.rehome(arena, store),
            Self::Branch { then_ops, else_ops, .. } => {
                then_ops.rehome(arena, store);
                else_ops.rehome(arena, store);
            }
            // Ternary arms stay as source ops so append_slice can flatten
            // them into JumpIfFalse/Jump, but nested MakeFunction/Loop
            // bodies still need a store range.
            Self::Conditional {
                consequent,
                alternate,
                ..
            } => {
                consequent.rehome_contents(arena, store);
                alternate.rehome_contents(arena, store);
            }
            Self::Try { body, handler, finalizer, .. } => {
                body.rehome(arena, store);
                rehome_optional(handler, arena, store);
                rehome_optional(finalizer, arena, store);
            }
            Self::Loop {
                init,
                test,
                body,
                update,
                label,
                per_iteration,
                ..
            } => {
                let flatten = label.is_none()
                    && per_iteration.is_empty()
                    && loop_parts_contain_call(init, test, body, update)
                    && !test
                        .source_ops()
                        .is_some_and(crate::machine::ops_contain_short_circuit)
                    && !test
                        .source_ops()
                        .is_some_and(crate::machine::test_always_true)
                    && !(init.source_ops().is_some_and(|ops| ops.is_empty())
                        && update.source_ops().is_some_and(|ops| ops.is_empty()))
                    && !init
                        .source_ops()
                        .is_some_and(crate::machine::ops_use_arguments)
                    && !test
                        .source_ops()
                        .is_some_and(crate::machine::ops_use_arguments)
                    && !body
                        .source_ops()
                        .is_some_and(crate::machine::ops_use_arguments)
                    && !update
                        .source_ops()
                        .is_some_and(crate::machine::ops_use_arguments);
                if flatten {
                    init.rehome_contents(arena, store);
                    test.rehome_contents(arena, store);
                    body.rehome_contents(arena, store);
                    update.rehome_contents(arena, store);
                } else {
                    rehome_loop(init, test, body, update, arena, store);
                }
            }
            Self::Switch { cases, .. } => rehome_cases(cases, arena, store),
            _ => {}
        }
    }

    pub(crate) fn body_count(&self) -> usize {
        let mut count = 0;
        self.visit_bodies(&mut |_| count += 1);
        count
    }

    pub(crate) fn visit_bodies(&self, visitor: &mut impl FnMut(&crate::machine::FunctionCode)) {
        match self {
            Self::AppendInstanceField(op) => {
                if let Some(initializer) = &op.initializer {
                    visitor(&initializer.body);
                }
            }
            Self::MakeFunction { body, .. }
            | Self::MakeFunctionWithKind { body, .. }
            | Self::Label { body, .. }
            | Self::With { body, .. }
            | Self::WithDispose { body, .. }
            | Self::PrivateScope { body, .. }
            | Self::StaticBlock { body, .. }
            | Self::IteratorBinding { body, .. }
            | Self::ForIn { body, .. }
            | Self::ForOf { body, .. } => visitor(body),
            Self::Branch { then_ops, else_ops, .. } => visit_pair(then_ops, else_ops, visitor),
            Self::Conditional { consequent, alternate, .. } => {
                visit_pair(consequent, alternate, visitor)
            }
            Self::Try { body, handler, finalizer, .. } => visit_try(body, handler, finalizer, visitor),
            Self::Loop { init, test, body, update, .. } => {
                visit_many([init, test, body, update], visitor)
            }
            Self::Switch { cases, .. } => cases.iter().for_each(|(test, body)| {
                if let Some(test) = test {
                    visitor(test);
                }
                visitor(body);
            }),
            _ => {}
        }
    }
}

fn loop_parts_contain_call(
    init: &crate::machine::FunctionCode,
    test: &crate::machine::FunctionCode,
    body: &crate::machine::FunctionCode,
    update: &crate::machine::FunctionCode,
) -> bool {
    [init, test, body, update]
        .into_iter()
        .any(|part| part.source_ops().is_some_and(crate::machine::ops_contain_call))
}

fn rehome_loop(
    init: &mut crate::machine::FunctionCode,
    test: &mut crate::machine::FunctionCode,
    body: &mut crate::machine::FunctionCode,
    update: &mut crate::machine::FunctionCode,
    arena: &mut crate::machine::CodeArena,
    store: &std::rc::Rc<std::sync::OnceLock<std::rc::Rc<crate::machine::CodeStore>>>,
) {
    for code in [init, test, body, update] { code.rehome(arena, store); }
}

fn rehome_cases(
    cases: &mut Vec<(Option<crate::machine::FunctionCode>, crate::machine::FunctionCode)>,
    arena: &mut crate::machine::CodeArena,
    store: &std::rc::Rc<std::sync::OnceLock<std::rc::Rc<crate::machine::CodeStore>>>,
) {
    for (test, body) in cases {
        if let Some(test) = test {
            test.rehome(arena, store);
        }
        body.rehome(arena, store);
    }
}

fn rehome_optional(
    body: &mut Option<crate::machine::FunctionCode>,
    arena: &mut crate::machine::CodeArena,
    store: &std::rc::Rc<std::sync::OnceLock<std::rc::Rc<crate::machine::CodeStore>>>,
) {
    if let Some(body) = body {
        body.rehome(arena, store);
    }
}

fn visit_pair(
    first: &crate::machine::FunctionCode,
    second: &crate::machine::FunctionCode,
    visitor: &mut impl FnMut(&crate::machine::FunctionCode),
) {
    visitor(first);
    visitor(second);
}

fn visit_many<const N: usize>(
    bodies: [&crate::machine::FunctionCode; N],
    visitor: &mut impl FnMut(&crate::machine::FunctionCode),
) {
    bodies.into_iter().for_each(visitor);
}

fn visit_try(
    body: &crate::machine::FunctionCode,
    handler: &Option<crate::machine::FunctionCode>,
    finalizer: &Option<crate::machine::FunctionCode>,
    visitor: &mut impl FnMut(&crate::machine::FunctionCode),
) {
    visitor(body);
    handler.iter().chain(finalizer).for_each(visitor);
}
