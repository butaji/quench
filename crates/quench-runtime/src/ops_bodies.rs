impl Op {
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
