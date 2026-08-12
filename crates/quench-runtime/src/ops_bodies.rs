impl Op {
    pub(crate) fn body_count(&self) -> usize {
        let mut count = 0;
        self.visit_bodies(&mut |_| count += 1);
        count
    }

    pub(crate) fn visit_bodies(&self, visitor: &mut impl FnMut(&crate::machine::FunctionCode)) {
        macro_rules! visit {
            ($($body:expr),+ $(,)?) => { $(visitor($body);)+ };
        }
        match self {
            Self::AppendInstanceField(op) => {
                if let Some(initializer) = &op.initializer {
                    visit!(&initializer.body);
                }
            }
            Self::MakeFunction { body, .. }
            | Self::MakeFunctionWithKind { body, .. }
            | Self::Label { body, .. }
            | Self::With { body, .. }
            | Self::PrivateScope { body, .. }
            | Self::IteratorBinding { body, .. }
            | Self::ForIn { body, .. }
            | Self::ForOf { body, .. } => {
                visit!(body);
            }
            Self::Branch { then_ops, else_ops, .. }
            | Self::Conditional {
                consequent: then_ops,
                alternate: else_ops,
                ..
            } => {
                visit!(then_ops, else_ops);
            }
            Self::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                visit!(body);
                if let Some(handler) = handler {
                    visit!(handler);
                }
                if let Some(finalizer) = finalizer {
                    visit!(finalizer);
                }
            }
            Self::Loop {
                init,
                test,
                body,
                update,
                ..
            } => {
                visit!(init, test, body, update);
            }
            Self::Switch { cases, .. } => {
                for (_, body) in cases {
                    visit!(body);
                }
            }
            _ => {}
        }
    }
}
