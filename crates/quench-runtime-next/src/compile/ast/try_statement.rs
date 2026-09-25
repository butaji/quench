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
        self.clear_statement_completion();
        let (slot, binding) = self.catch_slot(handler);
        let start = self.code.len() as u32;
        self.scoped_statements(&item.block.body);
        let end = self.code.len() as u32;
        let skip = self.emit(Op::Jump, 0, 0, 0, 0);
        let target = self.code.len() as u32;
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target,
            slot,
            return_target: None,
            return_slot: None,
            with_depth: self.with_depth,
        });
        self.clear_statement_completion();
        self.push_catch_binding(handler, binding);
        self.bind_catch_parameter(handler, binding);
        self.scoped_statements(&handler.body.body);
        self.lexical_scopes.pop();
        self.patch(skip);
    }

    fn try_finally_statement(&mut self, item: &TryStatement<'_>, finalizer: &BlockStatement<'_>) {
        self.clear_statement_completion();
        let error_atom = self.hidden_local("\0rqj:finally-error");
        let return_atom = self.hidden_local("\0rqj:finally-return");
        self.finally_contexts.push(FinallyContext {
            return_atom,
            return_edges: vec![],
            abrupt_edges: vec![],
        });
        let start = self.code.len() as u32;
        self.scoped_statements(&item.block.body);
        let context = self
            .finally_contexts
            .pop()
            .expect("try body finally context");
        let end = self.code.len() as u32;
        self.scoped_statements_without_completion(&finalizer.body);
        let normal_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let exceptional_target = self.code.len() as u32;
        self.scoped_statements_without_completion(&finalizer.body);
        let error = self.load_atom(error_atom);
        self.emit(Op::Throw, error, 0, 0, 0);
        let return_target = self.code.len() as u32;
        self.scoped_statements_without_completion(&finalizer.body);
        let return_value = self.load_atom(return_atom);
        self.emit(Op::Return, return_value, 0, 0, 0);
        self.patch_edges(&context.return_edges, return_target);
        self.emit_abrupt_paths(finalizer, &context);
        let end_target = self.code.len() as u32;
        self.patch_to(normal_exit, end_target);
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: exceptional_target,
            slot: self.local_slot(error_atom),
            return_target: Some(return_target),
            return_slot: self.local_slot(return_atom),
            with_depth: self.with_depth,
        });
    }

    fn try_catch_finally_statement(
        &mut self,
        item: &TryStatement<'_>,
        handler: &CatchClause<'_>,
        finalizer: &BlockStatement<'_>,
    ) {
        self.clear_statement_completion();
        let (catch_slot, binding) = self.catch_slot(handler);
        let error_atom = self.hidden_local("\0rqj:finally-error");
        let return_atom = self.hidden_local("\0rqj:finally-return");
        self.finally_contexts.push(FinallyContext {
            return_atom,
            return_edges: vec![],
            abrupt_edges: vec![],
        });
        let start = self.code.len() as u32;
        self.scoped_statements(&item.block.body);
        let end = self.code.len() as u32;
        let body_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let catch_target = self.code.len() as u32;
        let body_handler = self.handlers.len();
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: catch_target,
            slot: catch_slot,
            return_target: None,
            return_slot: None,
            with_depth: self.with_depth,
        });
        self.clear_statement_completion();
        self.push_catch_binding(handler, binding);
        self.bind_catch_parameter(handler, binding);
        let catch_start = self.code.len() as u32;
        self.scoped_statements(&handler.body.body);
        self.lexical_scopes.pop();
        let context = self
            .finally_contexts
            .pop()
            .expect("try body finally context");
        let catch_end = self.code.len() as u32;
        let catch_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let finalizer_target = self.code.len() as u32;
        self.scoped_statements_without_completion(&finalizer.body);
        let normal_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let exceptional_target = self.code.len() as u32;
        self.scoped_statements_without_completion(&finalizer.body);
        let error = self.load_atom(error_atom);
        self.emit(Op::Throw, error, 0, 0, 0);
        let return_target = self.code.len() as u32;
        self.scoped_statements_without_completion(&finalizer.body);
        let return_value = self.load_atom(return_atom);
        self.emit(Op::Return, return_value, 0, 0, 0);
        self.handlers[body_handler].return_target = Some(return_target);
        self.handlers[body_handler].return_slot = self.local_slot(return_atom);
        self.patch_to(body_exit, finalizer_target);
        self.patch_to(catch_exit, finalizer_target);
        self.patch_edges(&context.return_edges, return_target);
        self.emit_abrupt_paths(finalizer, &context);
        let end_target = self.code.len() as u32;
        self.patch_to(normal_exit, end_target);
        self.handlers.push(crate::bytecode::Handler {
            start: catch_start,
            end: catch_end,
            target: exceptional_target,
            slot: self.local_slot(error_atom),
            return_target: Some(return_target),
            return_slot: self.local_slot(return_atom),
            with_depth: self.with_depth,
        });
    }

    fn bind_catch_parameter(&mut self, handler: &CatchClause<'_>, binding: Option<Atom>) {
        if let Some(atom) = binding {
            let value = self.load_atom(atom);
            let parameter = handler.param.as_ref().unwrap();
            self.bind_pattern(&parameter.pattern, value);
        }
    }

    fn emit_abrupt_paths(&mut self, finalizer: &BlockStatement<'_>, context: &FinallyContext) {
        let mut paths = Vec::new();
        for abrupt in &context.abrupt_edges {
            if let Some((_, _, path, _)) = paths.iter().find(|(control, continue_edge, _, _)| {
                *control == abrupt.control && *continue_edge == abrupt.continue_edge
            }) {
                self.patch_to(abrupt.edge, *path);
                continue;
            }
            let path = self.code.len() as u32;
            self.scoped_statements_without_completion(&finalizer.body);
            let tail = self.emit(Op::Jump, 0, 0, 0, 0);
            paths.push((abrupt.control, abrupt.continue_edge, path, tail));
            self.patch_to(abrupt.edge, path);
        }
        for (control, continue_edge, _, tail) in paths {
            if continue_edge {
                self.controls[control].continues.push(tail);
            } else {
                self.controls[control].breaks.push(tail);
            }
        }
    }

    fn catch_slot(&mut self, handler: &CatchClause<'_>) -> (Option<u16>, Option<Atom>) {
        match &handler.param {
            Some(parameter)
                if matches!(&parameter.pattern, BindingPattern::BindingIdentifier(_)) =>
            {
                let BindingPattern::BindingIdentifier(_) = &parameter.pattern else {
                    unreachable!()
                };
                let binding = self.hidden_local("\0rqj:catch-binding");
                (self.local_slot(binding), Some(binding))
            }
            None => (None, None),
            Some(_) => {
                let atom = self.hidden_local("\0rqj:catch");
                (self.local_slot(atom), Some(atom))
            }
        }
    }

    pub(super) fn local_slot(&self, atom: Atom) -> Option<u16> {
        self.locals
            .iter()
            .position(|value| *value == atom)
            .map(|slot| slot as u16)
    }
}
