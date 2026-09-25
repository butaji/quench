use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn for_of_statement(&mut self, item: &ForOfStatement<'_>) {
        self.for_of_statement_labeled(item, None);
    }

    pub(super) fn for_of_statement_labeled(
        &mut self,
        item: &ForOfStatement<'_>,
        label: Option<Atom>,
    ) {
        self.clear_statement_completion();
        if item.r#await && !self.async_function {
            self.owner
                .reject(item.span, "for-await-of requires an async function");
            return;
        }
        let scoped = self.push_iteration_scope(&item.left);
        let source = self.expression(&item.right);
        self.for_iterable(&item.left, &item.body, source, item.r#await, label);
        if scoped {
            self.lexical_scopes.pop();
        }
    }

    pub(super) fn for_in_statement(&mut self, item: &ForInStatement<'_>) {
        self.for_in_statement_labeled(item, None);
    }

    pub(super) fn for_in_statement_labeled(
        &mut self,
        item: &ForInStatement<'_>,
        label: Option<Atom>,
    ) {
        self.clear_statement_completion();
        let scoped = self.push_iteration_scope(&item.left);
        let object = self.expression(&item.right);
        let keys = self.load_name("\0rqj:for-in-keys");
        let this = self.literal(Constant::Undefined);
        let base = self.next_reg;
        let argument = self.reg();
        self.emit(Op::Move, argument, object, 0, 0);
        let source = self.reg();
        self.emit(
            Op::Call,
            source,
            keys,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(base, 1, false, false),
        );
        self.for_iterable(&item.left, &item.body, source, false, label);
        if scoped {
            self.lexical_scopes.pop();
        }
    }

    fn for_iterable(
        &mut self,
        left: &ForStatementLeft<'_>,
        body: &Statement<'_>,
        source: Register,
        await_values: bool,
        label: Option<Atom>,
    ) {
        let iterator_atom = self.hidden_local("\0rqj:for-of:iterator");
        let iterator = self.reg();
        self.emit(
            if await_values {
                Op::GetAsyncIterator
            } else {
                Op::GetIterator
            },
            iterator,
            source,
            0,
            0,
        );
        self.store_atom(iterator_atom, iterator);
        let head = self.code.len() as u32;
        let iterator = self.load_atom(iterator_atom);
        let next_atom = self.owner.atom("next");
        let next_cache = self.owner.cache_site();
        let method_site = self.owner.method_sites.len() as u32;
        self.owner
            .method_sites
            .push((next_atom, next_cache, Vec::new(), None));
        let mut result = self.reg();
        self.emit(Op::CallMethod, result, iterator, 0, method_site);
        if await_values {
            let awaited = self.reg();
            self.emit(Op::Await, awaited, result, 0, 0);
            result = awaited;
        }
        let done = self.reg();
        let done_atom = self.owner.atom("done");
        let done_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            done,
            FieldBase::register(result).0,
            done_cache,
            done_atom,
        );
        let body_edge = self.emit(Op::JumpFalse, done, 0, 0, 0);
        let end_edge = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(body_edge);
        let value = self.reg();
        let value_atom = self.owner.atom("value");
        let value_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            value,
            FieldBase::register(result).0,
            value_cache,
            value_atom,
        );
        let using_iteration = match left {
            ForStatementLeft::VariableDeclaration(declaration)
                if matches!(
                    declaration.kind,
                    VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
                ) =>
            {
                Some(declaration.kind)
            }
            _ => None,
        };
        if using_iteration.is_some() {
            self.push_disposal_scope();
        }
        self.iterator_closures.push(iterator_atom);
        if matches!(
            left,
            ForStatementLeft::VariableDeclaration(declaration)
                if declaration.kind != VariableDeclarationKind::Var
        ) {
            self.emit(Op::CloneEnv, 0, 0, 0, 0);
        }
        self.bind_for_of_left(left, value);
        self.push_control(ControlKind::Loop, label);
        self.statement(body);
        let control = self.controls.pop().unwrap();
        self.iterator_closures.pop();
        let iteration_cleanup = self.code.len() as u32;
        self.patch_edges(&control.continues, iteration_cleanup);
        if using_iteration.is_some() {
            self.emit_disposal();
        }
        let skip_break_cleanup = self.emit(Op::Jump, 0, 0, 0, 0);
        let break_cleanup = self.code.len() as u32;
        self.patch_edges(&control.breaks, break_cleanup);
        if using_iteration.is_some() {
            self.emit_disposal();
            self.pop_disposal_scope();
        }
        let break_close = self.emit(Op::Jump, 0, 0, 0, 0);
        let update = self.code.len() as u32;
        self.patch_instruction(skip_break_cleanup, update);
        self.emit(Op::Jump, 0, 0, 0, head);
        let close = self.code.len() as u32;
        self.patch_instruction(break_close, close);
        let iterator = self.load_atom(iterator_atom);
        let close_fn = self.load_name("\0rqj:iterator-close");
        let ignored = self.reg();
        self.emit(Op::Call, ignored, close_fn, iterator, 0);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
    }

    fn push_iteration_scope(&mut self, left: &ForStatementLeft<'_>) -> bool {
        let ForStatementLeft::VariableDeclaration(declaration) = left else {
            return false;
        };
        if declaration.kind == VariableDeclarationKind::Var {
            return false;
        }
        let mut scope = FxHashMap::default();
        for item in &declaration.declarations {
            self.map_pattern_lexicals(&item.id, &mut scope);
        }
        self.push_lexical_bindings(scope);
        true
    }

    fn bind_for_of_left(&mut self, left: &ForStatementLeft<'_>, value: Register) {
        match left {
            ForStatementLeft::VariableDeclaration(declaration)
                if declaration.declarations.len() == 1 =>
            {
                let value = match declaration.kind {
                    VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => {
                        let asynchronous = declaration.kind == VariableDeclarationKind::AwaitUsing;
                        let stack = self.ensure_disposable_stack(asynchronous);
                        let method = if asynchronous { "useAsync" } else { "use" };
                        self.call_disposable_method(stack, method, value)
                    }
                    _ => value,
                };
                self.bind_pattern(&declaration.declarations[0].id, value);
            }
            _ => {
                if let Some(target) = left.as_simple_assignment_target() {
                    self.assign_target(target, value, 0);
                } else if let Some(target) = left.as_assignment_target() {
                    self.assign_pattern(target, value);
                } else {
                    self.owner
                        .reject(left.span(), "for-of assignment target unsupported");
                }
            }
        }
    }
}
