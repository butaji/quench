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
            Self::Branch { then_ops, else_ops, .. }
            | Self::Conditional {
                consequent: then_ops,
                alternate: else_ops,
                ..
            } => { visitor(then_ops); visitor(else_ops); }
            Self::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                visitor(body);
                if let Some(handler) = handler {
                    visitor(handler);
                }
                if let Some(finalizer) = finalizer {
                    visitor(finalizer);
                }
            }
            Self::Loop {
                init,
                test,
                body,
                update,
                ..
            } => { visitor(init); visitor(test); visitor(body); visitor(update); }
            Self::Switch { cases, .. } => {
                for (_, body) in cases {
                    visitor(body);
                }
            }
            _ => {}
        }
    }
}
