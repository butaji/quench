use super::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

struct IterationClosureFinder(bool);

impl<'a> Visit<'a> for IterationClosureFinder {
    fn visit_function(&mut self, _: &oxc_ast::ast::Function<'a>, _: ScopeFlags) {
        self.0 = true;
    }

    fn visit_arrow_function_expression(&mut self, _: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.0 = true;
    }

    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if matches!(&call.callee, oxc_ast::ast::Expression::Identifier(identifier) if identifier.name == "eval")
        {
            self.0 = true;
        } else {
            walk::walk_call_expression(self, call);
        }
    }
}

fn iteration_may_capture_bindings(
    left: &ForStatementLeft<'_>,
    right: &Expression<'_>,
    body: &Statement<'_>,
) -> bool {
    let mut finder = IterationClosureFinder(false);
    if let ForStatementLeft::VariableDeclaration(declaration) = left {
        finder.visit_variable_declaration(declaration);
    }
    finder.visit_expression(right);
    finder.visit_statement(body);
    finder.0
}

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
        let clone_environment = iteration_may_capture_bindings(&item.left, &item.right, &item.body);
        self.for_iterable(
            &item.left,
            &item.body,
            source,
            item.r#await,
            None,
            clone_environment,
            label,
        );
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
        let object_atom = self.hidden_local("\0rqj:for-in:source");
        self.store_atom(object_atom, object);
        let object = self.load_atom(object_atom);
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
        let clone_environment = iteration_may_capture_bindings(&item.left, &item.right, &item.body);
        self.for_iterable(
            &item.left,
            &item.body,
            source,
            false,
            Some(object_atom),
            clone_environment,
            label,
        );
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
        for_in_source: Option<Atom>,
        clone_environment: bool,
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
        let iterator = self.load_atom(iterator_atom);
        let next_atom = self.owner.atom("next");
        let next_cache = self.owner.cache_site();
        let next_method = self.reg();
        self.emit(
            Op::GetField,
            next_method,
            FieldBase::register(iterator).0,
            next_cache,
            next_atom,
        );
        let next_method_atom = self.hidden_local("\0rqj:for-of:next-method");
        self.store_atom(next_method_atom, next_method);
        let head = self.code.len() as u32;
        let iterator = self.load_atom(iterator_atom);
        let next_method = self.load_atom(next_method_atom);
        let mut result = self.reg();
        self.emit(Op::Call, result, next_method, iterator, 0);
        if await_values {
            let awaited = self.reg();
            self.emit(Op::Await, awaited, result, 0, 0);
            result = awaited;
        }
        self.emit(Op::RequireIteratorResult, 0, result, 0, 0);
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
        let invalid_for_in_key = for_in_source.map(|object_atom| {
            let validate = self.load_name("\0rqj:for-in-key-is-enumerable");
            let object = self.load_atom(object_atom);
            let base = self.next_reg;
            let object_arg = self.reg();
            self.emit(Op::Move, object_arg, object, 0, 0);
            let key_arg = self.reg();
            self.emit(Op::Move, key_arg, value, 0, 0);
            let valid = self.reg();
            let this = self.literal(Constant::Undefined);
            self.emit(
                Op::Call,
                valid,
                validate,
                this,
                crate::bytecode::ImmediateLayout::call_immediate(base, 2, false, false),
            );
            self.emit(Op::JumpFalse, valid, 0, 0, 0)
        });
        let iteration_body_start = self.code.len() as u32;
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
        self.iterator_closures.push(IteratorClosure {
            iterator: iterator_atom,
            control_depth: self.controls.len(),
        });
        if clone_environment
            && matches!(
                left,
                ForStatementLeft::VariableDeclaration(declaration)
                    if declaration.kind != VariableDeclarationKind::Var
            )
        {
            self.emit(Op::CloneEnv, 0, 0, 0, 0);
        }
        self.bind_for_of_left(left, value);
        self.push_control(ControlKind::Loop, label);
        self.statement(body);
        let iteration_body_end = self.code.len() as u32;
        self.emit_iterator_close_on_abrupt(iterator_atom, iteration_body_start, iteration_body_end);
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
        self.resolve_control_destinations(&control, break_cleanup, Some(iteration_cleanup));
        self.patch_instruction(skip_break_cleanup, update);
        self.emit(Op::Jump, 0, 0, 0, head);
        if let Some(edge) = invalid_for_in_key {
            self.patch_to(edge, head);
        }
        let close = self.code.len() as u32;
        self.patch_instruction(break_close, close);
        let iterator = self.load_atom(iterator_atom);
        let close_fn = self.load_name("\0rqj:iterator-close");
        let ignored = self.reg();
        self.emit(Op::Call, ignored, close_fn, iterator, 0);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
    }

    fn emit_iterator_close_on_abrupt(&mut self, iterator: Atom, start: u32, end: u32) {
        let error = self.hidden_local("\0rqj:for-of-body-error");
        let skip_cleanup = self.emit(Op::Jump, 0, 0, 0, 0);
        let cleanup = self.code.len() as u32;
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: cleanup,
            slot: self.local_slot(error),
            return_target: None,
            return_slot: None,
            with_depth: self.with_depth,
        });

        let close_iterator = self.load_atom(iterator);
        let _ = self.emit(Op::IteratorClose, 0, close_iterator, 0, 0);
        let close_end = self.code.len() as u32;
        let close_ok = self.emit(Op::Jump, 0, 0, 0, 0);
        let close_error = self.code.len() as u32;
        let ignored_error = self.hidden_local("\0rqj:for-of-close-error");
        self.handlers.push(crate::bytecode::Handler {
            start: cleanup,
            end: close_end,
            target: close_error,
            slot: self.local_slot(ignored_error),
            return_target: None,
            return_slot: None,
            with_depth: self.with_depth,
        });

        let original_error = self.load_atom(error);
        self.emit(Op::Throw, original_error, 0, 0, 0);
        let continuation = self.code.len() as u32;
        self.patch_to(skip_cleanup, continuation);
        self.patch_to(close_ok, close_error);
    }

    fn push_iteration_scope(&mut self, left: &ForStatementLeft<'_>) -> bool {
        let ForStatementLeft::VariableDeclaration(declaration) = left else {
            return false;
        };
        if declaration.kind == VariableDeclarationKind::Var {
            return false;
        }
        let mut scope = FxHashMap::default();
        let mut immutable = FxHashSet::default();
        self.map_declaration_lexicals(declaration, &mut scope, &mut immutable);
        self.push_immutable_lexical_bindings(scope, immutable);
        self.initialize_lexical_scope();
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
