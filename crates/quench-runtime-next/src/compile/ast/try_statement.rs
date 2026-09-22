use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn try_statement(&mut self, item: &TryStatement<'_>) {
        if let Some(finalizer) = &item.finalizer {
            if let Some(handler) = &item.handler {
                self.try_catch_finally_statement(item, handler, finalizer);
            } else {
                self.try_finally_statement(item, finalizer);
            }
            return;
        }
        let Some(handler) = &item.handler else {
            self.owner
                .reject(item.span, "try without catch is unsupported");
            return;
        };
        let (slot, binding) = self.catch_slot(handler);
        let start = self.code.len() as u32;
        self.statements(&item.block.body);
        let end = self.code.len() as u32;
        let skip = self.emit(Op::Jump, 0, 0, 0, 0);
        let target = self.code.len() as u32;
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target,
            slot,
        });
        self.bind_catch_parameter(handler, binding);
        self.statements(&handler.body.body);
        self.patch(skip);
    }

    fn try_finally_statement(&mut self, item: &TryStatement<'_>, finalizer: &BlockStatement<'_>) {
        let error_atom = self.hidden_local("\0rqj:finally-error");
        let start = self.code.len() as u32;
        self.statements(&item.block.body);
        let end = self.code.len() as u32;
        self.statements(&finalizer.body);
        let normal_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let exceptional_target = self.code.len() as u32;
        self.statements(&finalizer.body);
        let error = self.load_atom(error_atom);
        self.emit(Op::Throw, error, 0, 0, 0);
        let end_target = self.code.len() as u32;
        self.patch_to(normal_exit, end_target);
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: exceptional_target,
            slot: self.local_slot(error_atom),
        });
    }

    fn try_catch_finally_statement(
        &mut self,
        item: &TryStatement<'_>,
        handler: &CatchClause<'_>,
        finalizer: &BlockStatement<'_>,
    ) {
        let (catch_slot, binding) = self.catch_slot(handler);
        let error_atom = self.hidden_local("\0rqj:finally-error");
        let start = self.code.len() as u32;
        self.statements(&item.block.body);
        let end = self.code.len() as u32;
        let body_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let catch_target = self.code.len() as u32;
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: catch_target,
            slot: catch_slot,
        });
        self.bind_catch_parameter(handler, binding);
        let catch_start = self.code.len() as u32;
        self.statements(&handler.body.body);
        let catch_end = self.code.len() as u32;
        let catch_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let finalizer_target = self.code.len() as u32;
        self.statements(&finalizer.body);
        let normal_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let exceptional_target = self.code.len() as u32;
        self.statements(&finalizer.body);
        let error = self.load_atom(error_atom);
        self.emit(Op::Throw, error, 0, 0, 0);
        let end_target = self.code.len() as u32;
        self.patch_to(body_exit, finalizer_target);
        self.patch_to(catch_exit, finalizer_target);
        self.patch_to(normal_exit, end_target);
        self.handlers.push(crate::bytecode::Handler {
            start: catch_start,
            end: catch_end,
            target: exceptional_target,
            slot: self.local_slot(error_atom),
        });
    }

    fn bind_catch_parameter(&mut self, handler: &CatchClause<'_>, binding: Option<Atom>) {
        if let Some(atom) = binding {
            let value = self.load_atom(atom);
            let parameter = handler.param.as_ref().unwrap();
            self.bind_pattern(&parameter.pattern, value);
        }
    }

    fn catch_slot(&mut self, handler: &CatchClause<'_>) -> (Option<u16>, Option<Atom>) {
        match &handler.param {
            Some(parameter)
                if matches!(&parameter.pattern, BindingPattern::BindingIdentifier(_)) =>
            {
                let BindingPattern::BindingIdentifier(id) = &parameter.pattern else {
                    unreachable!()
                };
                let atom = self.owner.atom(id.name.as_str());
                (self.local_slot(atom), None)
            }
            None => (None, None),
            Some(_) => {
                let atom = self.hidden_local("\0rqj:catch");
                (self.local_slot(atom), Some(atom))
            }
        }
    }

    fn local_slot(&self, atom: Atom) -> Option<u16> {
        self.locals
            .iter()
            .position(|value| *value == atom)
            .map(|slot| slot as u16)
    }
}
