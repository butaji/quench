impl Op {
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
            | Self::PrivateScope { body, .. }
            | Self::IteratorBinding { body, .. }
            | Self::ForIn { body, .. }
            | Self::ForOf { body, .. } => body.rehome(arena, store),
            Self::Branch { then_ops, else_ops, .. }
            | Self::Conditional { consequent: then_ops, alternate: else_ops, .. } => {
                then_ops.rehome(arena, store);
                else_ops.rehome(arena, store);
            }
            Self::Try { body, handler, finalizer, .. } => {
                body.rehome(arena, store);
                rehome_optional(handler, arena, store);
                rehome_optional(finalizer, arena, store);
            }
            Self::Loop { init, test, body, update, .. } => rehome_loop(init, test, body, update, arena, store),
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
            | Self::PrivateScope { body, .. }
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
            Self::Switch { cases, .. } => cases.iter().for_each(|(_, body)| visitor(body)),
            _ => {}
        }
    }
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
    cases: &mut Vec<(Option<Constant>, crate::machine::FunctionCode)>,
    arena: &mut crate::machine::CodeArena,
    store: &std::rc::Rc<std::sync::OnceLock<std::rc::Rc<crate::machine::CodeStore>>>,
) {
    for (_, body) in cases { body.rehome(arena, store); }
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
