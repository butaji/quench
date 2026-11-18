use super::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

/// Finds whether a loop may make a closure that outlives the iteration it captured, so the
/// loop's bindings need a copy of the environment per iteration (`CloneEnv`). A closure that is
/// the initial value of a `let` or `const` declared in the loop's body, whose binding is only
/// ever called (`Function::prototype_unobservable`), and that makes no closure itself, dies with
/// its iteration in strict code: the binding is the value's only home and is out of scope at the
/// next iteration, and no legacy `caller` hands it out. It reads the iteration's bindings while
/// they are current, so it needs no copy.
struct IterationClosureFinder<'s> {
    found: bool,
    /// The closures that may die with their iteration (`Compiler::prototype_unobservable`),
    /// none in sloppy code.
    call_only: Option<&'s FxHashSet<u32>>,
    /// Inside the loop's head, whose bindings each iteration copies: any closure there counts.
    head: bool,
}

impl<'s> IterationClosureFinder<'s> {
    fn new(call_only: Option<&'s FxHashSet<u32>>) -> Self {
        Self { found: false, call_only, head: true }
    }

    fn body(&mut self, body: &Statement<'_>) {
        self.head = false;
        self.visit_statement(body);
        self.head = true;
    }

    /// Whether `value`, a declarator's initial value, is a closure that dies with its iteration.
    fn dies_with_iteration(&self, value: &Expression<'_>) -> bool {
        let Some(call_only) = self.call_only else { return false };
        // Its body makes no closure (nor evaluates code that could): visited as a loop's whole
        // head, any closure inside it counts.
        let mut inner = IterationClosureFinder::new(None);
        match value.without_parentheses() {
            Expression::FunctionExpression(function) if call_only.contains(&function.span.start) => {
                inner.visit_formal_parameters(&function.params);
                if let Some(body) = &function.body {
                    inner.visit_function_body(body);
                }
            }
            Expression::ArrowFunctionExpression(arrow) if call_only.contains(&arrow.span.start) => {
                inner.visit_formal_parameters(&arrow.params);
                inner.visit_arrow_function_body(&arrow.body);
            }
            _ => return false,
        }
        !inner.found
    }
}

impl<'a> Visit<'a> for IterationClosureFinder<'_> {
    fn visit_function(&mut self, _: &oxc_ast::ast::Function<'a>, _: ScopeFlags) {
        self.found = true;
    }

    fn visit_arrow_function_expression(&mut self, _: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.found = true;
    }

    fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {
        self.found = true;
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        let lexical = matches!(it.kind, VariableDeclarationKind::Let | VariableDeclarationKind::Const);
        for declarator in &it.declarations {
            let plain = matches!(declarator.id, oxc_ast::ast::BindingPattern::BindingIdentifier(_));
            match &declarator.init {
                Some(init) if !self.head && lexical && plain && self.dies_with_iteration(init) => {}
                _ => self.visit_variable_declarator(declarator),
            }
        }
    }

    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if super::super::early::is_direct_eval_call(call) {
            self.found = true;
        } else {
            walk::walk_call_expression(self, call);
        }
    }
}

fn iteration_may_capture_bindings(
    left: &ForStatementLeft<'_>,
    right: &Expression<'_>,
    body: &Statement<'_>,
    call_only: Option<&FxHashSet<u32>>,
) -> bool {
    let mut finder = IterationClosureFinder::new(call_only);
    if let ForStatementLeft::VariableDeclaration(declaration) = left {
        finder.visit_variable_declaration(declaration);
    }
    finder.visit_expression(right);
    finder.body(body);
    finder.found
}

/// Whether a `for (let ...; test; update) body` loop may make a closure that outlives the
/// iteration it captured (`IterationClosureFinder`).
pub(super) fn for_loop_may_capture_bindings(item: &ForStatement<'_>, call_only: Option<&FxHashSet<u32>>) -> bool {
    let mut finder = IterationClosureFinder::new(call_only);
    if let Some(init) = &item.init {
        finder.visit_for_statement_init(init);
    }
    if let Some(test) = &item.test {
        finder.visit_expression(test);
    }
    if let Some(update) = &item.update {
        finder.visit_expression(update);
    }
    finder.body(&item.body);
    finder.found
}

impl FunctionCompiler<'_, '_> {
    /// The closures that may die with their iteration (`IterationClosureFinder`): in strict code
    /// alone, where no legacy `caller` hands one out.
    pub(super) fn iteration_call_only(&self) -> Option<&FxHashSet<u32>> {
        self.strict.then_some(&self.owner.prototype_unobservable)
    }

    pub(super) fn close_iterator(&mut self, iterator: Register) -> Register {
        // IteratorClose produces a result even when a synchronous caller
        // discards it. Register zero may hold a promoted parameter/local.
        let result = self.reg();
        self.emit(Op::IteratorClose, result, iterator, 0, 0);
        result
    }

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
        let clone_environment = iteration_may_capture_bindings(&item.left, &item.right, &item.body, self.iteration_call_only());
        self.for_iterable(
            &item.left,
            &item.body,
            source,
            item.r#await,
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
        if let ForStatementLeft::VariableDeclaration(declaration) = &item.left
            && declaration.kind == VariableDeclarationKind::Var
        {
            for item in &declaration.declarations {
                if let Some(initializer) = &item.init {
                    let value = self.initializer_value(initializer, &item.id);
                    self.bind_pattern(&item.id, value);
                }
            }
        }
        let object = self.expression(&item.right);
        let state = self.reg();
        self.emit(Op::ForInStart, state, object, 0, 0);
        let clone_environment = iteration_may_capture_bindings(&item.left, &item.right, &item.body, self.iteration_call_only());
        let listed = match &item.right { Expression::Identifier(name) => Some(name.name.as_str()), _ => None };
        self.for_in_keys(&item.left, &item.body, (state, listed), clone_environment, label);
        if scoped {
            self.lexical_scopes.pop();
        }
    }

    /// Steps the for-in `state` (`Op::ForInNext`), which yields each snapshot key still an
    /// enumerable property: the snapshot is an internal array no script reaches, so no
    /// iterator protocol runs and no abrupt exit has anything to close.
    fn for_in_keys(
        &mut self,
        left: &ForStatementLeft<'_>,
        body: &Statement<'_>,
        (state, listed): (Register, Option<&str>),
        clone_environment: bool,
        label: Option<Atom>,
    ) {
        let state_atom = self.hidden_local("\0quench:for-in:state");
        self.store_atom(state_atom, state);
        let head = self.code.len() as u32;
        let state = self.load_atom(state_atom);
        let key = self.reg();
        let end_edge = self.emit(Op::ForInNext, key, state, 0, 0);
        self.push_control(ControlKind::Loop, label);
        if clone_environment
            && matches!(left, ForStatementLeft::VariableDeclaration(declaration)
                if declaration.kind != VariableDeclarationKind::Var)
        {
            self.clone_lexical_environment();
        }
        self.bind_iteration_left(left, key);
        // `o[k]` in the body of `for (k in o)` reads through the loop's state (`Op::GetEnumerated`).
        let enumeration = listed.zip(iteration_key_name(left)).map(|(object, key)| super::Enumeration {
            object: object.to_owned(), key: key.to_owned(), state: state_atom });
        let enumerated = enumeration.is_some();
        self.enumerations.extend(enumeration);
        self.statement(body);
        if enumerated {
            self.enumerations.pop();
        }
        let control = self.controls.pop().unwrap();
        let update = self.code.len() as u32;
        self.patch_edges(&control.continues, update);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
        self.patch_edges(&control.breaks, end);
        self.resolve_control_destinations(&control, end, Some(update));
    }

    /// Whether each value a for-of loop over `left` takes goes only to an array pattern that
    /// binds names: `const`, `let` or `var` of one plain array pattern of names and holes,
    /// with no rest. The pattern reads the value and nothing keeps it, so a step may yield the
    /// one shared entry pair (`Vm::iterator_item_value`); the loop marks its
    /// `RequireIteratorResult` with that.
    fn value_only_to_array_pattern(left: &ForStatementLeft<'_>) -> bool {
        let ForStatementLeft::VariableDeclaration(declaration) = left else {
            return false;
        };
        if super::disposal::disposes(declaration.kind) || declaration.declarations.len() != 1 {
            return false;
        }
        let BindingPattern::ArrayPattern(array) = &declaration.declarations[0].id else {
            return false;
        };
        array.rest.is_none()
            && !array.elements.is_empty()
            && array.elements.iter().all(|element| matches!(element, None | Some(BindingPattern::BindingIdentifier(_))))
    }

    fn for_iterable(
        &mut self,
        left: &ForStatementLeft<'_>,
        body: &Statement<'_>,
        source: Register,
        await_values: bool,
        clone_environment: bool,
        label: Option<Atom>,
    ) {
        let iterator_atom = self.hidden_local("\0quench:for-of:iterator");
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
        let next_method_atom = self.hidden_local("\0quench:for-of:next-method");
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
        let pattern_only = u16::from(!await_values && Self::value_only_to_array_pattern(left));
        self.emit(Op::RequireIteratorResult, pattern_only, result, 0, 0);
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
        let iteration_body_start = self.code.len() as u32;
        self.emit(Op::IteratorCleanupPush, iterator, done, 0, 0);
        let using_iteration = match left {
            ForStatementLeft::VariableDeclaration(declaration)
                if super::disposal::disposes(declaration.kind) =>
            {
                Some(declaration.kind)
            }
            _ => None,
        };
        self.iterator_closures.push(IteratorClosure {
            iterator: iterator_atom,
            asynchronous: await_values,
            control_depth: self.controls.len(),
        });
        self.push_control(ControlKind::Loop, label);
        let disposal_error = using_iteration.map(|_| {
            self.push_disposal_scope();
            self.push_disposal_context();
            self.hidden_local("\0quench:for-of-using-error")
        });
        if clone_environment
            && matches!(
                left,
                ForStatementLeft::VariableDeclaration(declaration)
                    if declaration.kind != VariableDeclarationKind::Var
            )
        {
            self.clone_lexical_environment();
        }
        // The iterator cleanup record reads `iterator` and `done` until the body ends.
        self.with_registers_held(|compiler| {
            compiler.bind_iteration_left(left, value);
            compiler.statement(body);
        });
        if let Some(error) = disposal_error {
            let end = self.code.len() as u32;
            let context = self
                .finally_contexts
                .pop()
                .expect("iteration disposal context");
            self.emit_disposal_scope_exit(iteration_body_start, end, error, Some(context));
            self.pop_disposal_scope();
        }
        let iteration_body_end = self.code.len() as u32;
        self.emit_iterator_close_on_abrupt(
            iterator_atom,
            iteration_body_start,
            iteration_body_end,
            await_values,
        );
        let control = self.controls.pop().unwrap();
        self.iterator_closures.pop();
        let iteration_cleanup = self.code.len() as u32;
        self.patch_edges(&control.continues, iteration_cleanup);
        self.emit(Op::IteratorCleanupPop, 0, 0, 0, 0);
        let skip_break_cleanup = self.emit(Op::Jump, 0, 0, 0, 0);
        let break_cleanup = self.code.len() as u32;
        self.patch_edges(&control.breaks, break_cleanup);
        self.emit(Op::IteratorCleanupPop, 0, 0, 0, 0);
        let break_close = self.emit(Op::Jump, 0, 0, 0, 0);
        let update = self.code.len() as u32;
        self.resolve_control_destinations(&control, break_cleanup, Some(iteration_cleanup));
        self.patch_instruction(skip_break_cleanup, update);
        self.emit(Op::Jump, 0, 0, 0, head);
        let close = self.code.len() as u32;
        self.patch_instruction(break_close, close);
        let iterator = self.load_atom(iterator_atom);
        let close_fn = self.load_name(crate::bytecode::ITERATOR_CLOSE_BINDING);
        let ignored = self.reg();
        self.emit(Op::Call, ignored, close_fn, iterator, 0);
        if await_values {
            let awaited = self.reg();
            self.emit(Op::Await, awaited, ignored, 0, 0);
        }
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
    }

    fn emit_iterator_close_on_abrupt(
        &mut self,
        iterator: Atom,
        start: u32,
        end: u32,
        await_values: bool,
    ) {
        let error = self.hidden_local("\0quench:for-of-body-error");
        let skip_cleanup = self.emit(Op::Jump, 0, 0, 0, 0);
        let cleanup = self.code.len() as u32;
        let exclusions = self
            .iterator_close_ranges
            .iter()
            .copied()
            .filter(|(excluded_start, excluded_end)| {
                start <= *excluded_start && *excluded_end <= end
            })
            .collect::<Vec<_>>();
        let slot = self.local_slot(error);
        let with_depth = self.with_depth;

        self.emit(Op::IteratorCleanupPop, 0, 0, 0, 0);
        let close_iterator = self.load_atom(iterator);
        let close_result = self.close_iterator(close_iterator);
        if await_values {
            let awaited = self.reg();
            self.emit(Op::Await, awaited, close_result, 0, 0);
        }
        let close_end = self.code.len() as u32;
        let close_ok = self.emit(Op::Jump, 0, 0, 0, 0);
        let close_error = self.code.len() as u32;
        let ignored_error = self.hidden_local("\0quench:for-of-close-error");
        self.handlers.push(crate::bytecode::Handler {
            start: cleanup,
            end: close_end,
            target: close_error,
            slot: self.local_slot(ignored_error).map(crate::bytecode::ThrownValueSlot::Local),
            return_target: None,
            return_slot: None,
            with_depth: self.with_depth,
        });

        let original_error = self.load_atom(error);
        self.emit(Op::Throw, original_error, 0, 0, 0);
        let (return_target, return_slot) = if self.generator {
            let return_atom = self.hidden_local("\0quench:for-of-body-return");
            let return_target = self.code.len() as u32;
            self.emit(Op::IteratorCleanupPop, 0, 0, 0, 0);
            let close_iterator = self.load_atom(iterator);
            let close_result = self.close_iterator(close_iterator);
            if await_values {
                let awaited = self.reg();
                self.emit(Op::Await, awaited, close_result, 0, 0);
            }
            // This loop has closed. Compile the original return through the
            // remaining loop/finally owners, as for an explicit return statement.
            let closure = self.iterator_closures.pop().expect("loop cleanup owner");
            let value = self.load_atom(return_atom);
            self.emit_return(value);
            self.iterator_closures.push(closure);
            (Some(return_target), self.local_slot(return_atom))
        } else {
            (None, None)
        };
        self.push_handlers_excluding(start, end, &exclusions, |start, end| {
            crate::bytecode::Handler {
                start,
                end,
                target: cleanup,
                slot: slot.map(crate::bytecode::ThrownValueSlot::Local),
                return_target,
                return_slot,
                with_depth,
            }
        });
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

    fn bind_iteration_left(&mut self, left: &ForStatementLeft<'_>, value: Register) {
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
                if let Some(target) = left.as_assignment_target() {
                    self.assign_pattern(target, value);
                } else {
                    self.owner
                        .reject(left.span(), "iteration assignment target unsupported");
                }
            }
        }
    }
}

/// The name a for-in's left side binds each key to, where it is one identifier (`for (k in o)`,
/// `for (const k in o)`).
fn iteration_key_name<'s>(left: &'s ForStatementLeft<'_>) -> Option<&'s str> {
    match left {
        ForStatementLeft::VariableDeclaration(declaration) => match declaration.declarations.as_slice() {
            [declarator] => match &declarator.id {
                BindingPattern::BindingIdentifier(name) => Some(name.name.as_str()),
                _ => None,
            },
            _ => None,
        },
        ForStatementLeft::AssignmentTargetIdentifier(name) => Some(name.name.as_str()),
        _ => None,
    }
}
