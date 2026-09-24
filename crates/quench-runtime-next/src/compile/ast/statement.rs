use super::*;

const STRICT_EQUAL_OPERATOR: u32 = 2;

fn is_anonymous_function_definition(mut expression: &Expression<'_>) -> bool {
    while let Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    match expression {
        Expression::FunctionExpression(function) => function.id.is_none(),
        Expression::ArrowFunctionExpression(_) => true,
        Expression::ClassExpression(class) => class.id.is_none(),
        _ => false,
    }
}

impl FunctionCompiler<'_, '_> {
    pub(crate) fn statements(&mut self, body: &[Statement<'_>]) {
        for statement in body {
            self.statement(statement);
            self.release_temporaries();
        }
    }

    pub(super) fn statement(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::EmptyStatement(_) | Statement::FunctionDeclaration(_) => {}
            Statement::ExportDeclaration(item) => match &item.declaration {
                Declaration::VariableDeclaration(declaration) => self.variables(declaration),
                Declaration::ClassDeclaration(declaration) => {
                    self.class_declaration(declaration);
                }
                Declaration::FunctionDeclaration(_) => {}
                _ => self.owner.reject(
                    item.span,
                    "export declaration is outside the supported subset",
                ),
            },
            Statement::ExportNamedDeclaration(_)
            | Statement::ExportFromDeclaration(_)
            | Statement::ExportAllDeclaration(_) => {}
            Statement::ImportDeclaration(item) if item.phase.is_none() => {}
            Statement::ImportDeclaration(item)
                if matches!(item.phase, Some(ImportPhase::Defer))
                    && item.specifiers.as_ref().is_some_and(|specifiers| {
                        specifiers.iter().all(|specifier| {
                            matches!(
                                specifier,
                                ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)
                            )
                        })
                    }) => {}
            Statement::ImportDeclaration(item) => self.owner.reject(
                item.span,
                "deferred and source imports are outside the supported subset",
            ),
            Statement::ExportDefaultDeclaration(item) => match &item.declaration {
                oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(_) => {}
                oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    let value = self.class_expression(class);
                    if class.id.is_none() {
                        let name = self.owner.atom("default");
                        self.emit(Op::SetFunctionName, value, 0, 0, name);
                    } else if let Some(identifier) = &class.id {
                        let atom = self.owner.atom(identifier.name.as_str());
                        self.store_atom(atom, value);
                    }
                    let binding = super::super::module_default_binding(self.owner.source);
                    let atom = self.owner.atom(&binding);
                    self.store_atom(atom, value);
                }
                _ => {
                    if let Some(expression) = item.declaration.as_expression() {
                        let value = self.expression(expression);
                        if is_anonymous_function_definition(expression) {
                            let name = self.owner.atom("default");
                            self.emit(Op::SetFunctionName, value, 0, 0, name);
                        }
                        let binding = super::super::module_default_binding(self.owner.source);
                        let atom = self.owner.atom(&binding);
                        self.store_atom(atom, value);
                    } else {
                        self.owner.reject(
                            item.span,
                            "default class exports are outside the supported subset",
                        );
                    }
                }
            },
            Statement::ClassDeclaration(item) => {
                self.class_declaration(item);
            }
            Statement::ExpressionStatement(item) => {
                self.expression(&item.expression);
            }
            Statement::BlockStatement(block) => {
                let has_using = block.body.iter().any(|statement| {
                    matches!(
                        statement,
                        Statement::VariableDeclaration(declaration)
                            if matches!(
                                declaration.kind,
                                VariableDeclarationKind::Using
                                    | VariableDeclarationKind::AwaitUsing
                            )
                    )
                });
                self.push_lexical_scope(&block.body);
                self.emit_hoisted(&block.body);
                self.statements(&block.body);
                self.lexical_scopes.pop();
                if has_using {
                    self.emit_disposal();
                }
            }
            Statement::VariableDeclaration(item) => self.variables(item),
            Statement::ReturnStatement(item) => self.return_statement(item),
            Statement::IfStatement(item) => self.if_statement(item),
            Statement::WhileStatement(item) => self.while_statement(item),
            Statement::DoWhileStatement(item) => self.do_while_statement(item),
            Statement::ForStatement(item) => self.for_statement(item),
            Statement::ForInStatement(item) => self.for_in_statement(item),
            Statement::ForOfStatement(item) => self.for_of_statement(item),
            Statement::SwitchStatement(item) => self.switch_statement(item),
            Statement::BreakStatement(item) => self.break_statement(item),
            Statement::ContinueStatement(item) => self.continue_statement(item),
            Statement::LabeledStatement(item) => self.labeled_statement(item),
            Statement::ThrowStatement(item) => {
                let value = self.expression(&item.argument);
                self.close_active_iterators();
                self.emit(Op::Throw, value, 0, 0, 0);
            }
            Statement::TryStatement(item) => self.try_statement(item),
            Statement::WithStatement(item) => self.with_statement(item),
            _ => self.owner.reject(
                statement.span(),
                "statement is outside the supported subset",
            ),
        }
    }

    fn with_statement(&mut self, item: &WithStatement<'_>) {
        let object = self.expression(&item.object);
        let enter = self.load_name("\0rqj:with-enter");
        let argument = self.reg();
        self.emit(Op::Move, argument, object, 0, 0);
        let ignored = self.reg();
        self.emit(
            Op::Call,
            ignored,
            enter,
            enter,
            crate::bytecode::ImmediateLayout::call_immediate(argument, 1, false, false),
        );
        self.with_depth = self.with_depth.saturating_add(1);
        self.statement(&item.body);
        self.with_depth = self.with_depth.saturating_sub(1);
        let exit = self.load_name("\0rqj:with-exit");
        let ignored = self.reg();
        self.emit(Op::Call, ignored, exit, exit, 0);
    }
    fn return_statement(&mut self, item: &ReturnStatement<'_>) {
        if let Some(value) = &item.argument {
            self.return_expression(value);
        } else {
            let value = self.literal(Constant::Undefined);
            self.emit_return(value);
        }
    }
    fn close_active_iterators(&mut self) {
        if self.iterator_closures.is_empty() {
            return;
        }
        let close_fn = self.load_name("\0rqj:iterator-close");
        let iterators = self.iterator_closures.clone();
        for atom in iterators.into_iter().rev() {
            let iterator = self.load_atom(atom);
            let ignored = self.reg();
            self.emit(Op::Call, ignored, close_fn, iterator, 0);
        }
    }
    fn return_expression(&mut self, expression: &Expression<'_>) {
        if !self.finally_contexts.is_empty() {
            let value = self.expression(expression);
            self.emit_return(value);
            return;
        }
        match expression {
            Expression::LogicalExpression(value) => self.return_logical(value),
            Expression::ConditionalExpression(value) => {
                let alternate = self.condition(&value.test);
                self.return_expression(&value.consequent);
                self.patch(alternate);
                self.return_expression(&value.alternate);
            }
            _ => {
                let value = self.expression(expression);
                self.emit_return(value);
            }
        }
    }
    fn emit_return(&mut self, value: Register) {
        self.close_active_iterators();
        let Some(context) = self.finally_contexts.last() else {
            self.emit(Op::Return, value, 0, 0, 0);
            return;
        };
        let return_atom = context.return_atom;
        self.store_atom(return_atom, value);
        let edge = self.emit(Op::Jump, 0, 0, 0, 0);
        self.finally_contexts
            .last_mut()
            .expect("finally context remains active")
            .return_edges
            .push(edge);
    }

    fn return_logical(&mut self, value: &LogicalExpression<'_>) {
        let left = self.expression(&value.left);
        if value.operator.is_coalesce() {
            let null = self.literal(Constant::Null);
            let is_null = self.emit_binary(0, Operand::register(left), Operand::register(null));
            let check_undefined = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
            self.return_expression(&value.right);
            self.patch(check_undefined);

            let undefined = self.literal(Constant::Undefined);
            let is_undefined =
                self.emit_binary(0, Operand::register(left), Operand::register(undefined));
            let return_left = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);
            self.return_expression(&value.right);
            self.patch(return_left);
            self.emit_return(left);
            return;
        }
        let false_edge = self.emit(Op::JumpFalse, left, 0, 0, 0);
        if value.operator.is_or() {
            self.emit_return(left);
            self.patch(false_edge);
            self.return_expression(&value.right);
        } else {
            self.return_expression(&value.right);
            self.patch(false_edge);
            self.emit_return(left);
        }
    }

    fn variables(&mut self, declaration: &VariableDeclaration<'_>) {
        if matches!(
            declaration.kind,
            VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
        ) {
            if declaration.kind == VariableDeclarationKind::AwaitUsing && !self.async_function {
                self.owner
                    .reject(declaration.span, "await using requires an async function");
                return;
            }
            let stack = self.ensure_disposable_stack();
            for item in &declaration.declarations {
                let value = if let Some(init) = &item.init {
                    self.expression(init)
                } else {
                    self.literal(Constant::Undefined)
                };
                let method = if declaration.kind == VariableDeclarationKind::AwaitUsing {
                    "useAsync"
                } else {
                    "use"
                };
                let registered = self.call_disposable_method(stack, method, value);
                self.bind_pattern(&item.id, registered);
            }
            return;
        }
        for item in &declaration.declarations {
            if let Some(init) = &item.init {
                let value = match (init, &item.id) {
                    (
                        Expression::ClassExpression(class),
                        BindingPattern::BindingIdentifier(identifier),
                    ) if class.id.is_none() => {
                        self.named_class_expression(class, identifier.name.as_str())
                    }
                    _ => self.expression(init),
                };
                if Self::anonymous_function_definition(init)
                    && let BindingPattern::BindingIdentifier(identifier) = &item.id
                {
                    let name = self.owner.atom(identifier.name.as_str());
                    self.emit(Op::SetFunctionName, value, 0, 0, name);
                }
                self.bind_pattern(&item.id, value);
            } else if declaration.kind == VariableDeclarationKind::Let {
                // `let x;` initializes the binding to undefined at this point.
                // Leaving the slot in its hoisted TDZ state makes later
                // expressions (including computed class keys) observe a
                // spurious ReferenceError.
                let value = self.literal(Constant::Undefined);
                self.bind_pattern(&item.id, value);
            }
        }
    }

    fn if_statement(&mut self, item: &IfStatement<'_>) {
        let alternate = self.condition(&item.test);
        self.statement(&item.consequent);
        if let Some(other) = &item.alternate {
            let end = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(alternate);
            self.statement(other);
            self.patch(end);
        } else {
            self.patch(alternate);
        }
    }

    fn while_statement(&mut self, item: &WhileStatement<'_>) {
        self.while_statement_labeled(item, None);
    }

    fn while_statement_labeled(&mut self, item: &WhileStatement<'_>, label: Option<Atom>) {
        let head = self.code.len() as u32;
        let condition_end = self.condition(&item.test);
        self.push_control(ControlKind::Loop, label);
        self.statement(&item.body);
        let control = self.controls.pop().unwrap();
        self.patch_edges(&control.continues, head);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(condition_end, end);
        self.patch_edges(&control.breaks, end);
    }

    fn do_while_statement(&mut self, item: &DoWhileStatement<'_>) {
        self.do_while_statement_labeled(item, None);
    }

    fn do_while_statement_labeled(&mut self, item: &DoWhileStatement<'_>, label: Option<Atom>) {
        let head = self.code.len() as u32;
        self.push_control(ControlKind::Loop, label);
        self.statement(&item.body);
        let control = self.controls.pop().unwrap();
        let condition = self.code.len() as u32;
        self.patch_edges(&control.continues, condition);
        let end_edge = self.condition(&item.test);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
        self.patch_edges(&control.breaks, end);
    }

    fn for_statement(&mut self, item: &ForStatement<'_>) {
        self.for_statement_labeled(item, None);
    }

    fn for_statement_labeled(&mut self, item: &ForStatement<'_>, label: Option<Atom>) {
        let scoped = match item.init.as_ref() {
            Some(ForStatementInit::VariableDeclaration(declaration))
                if matches!(
                    declaration.kind,
                    VariableDeclarationKind::Let | VariableDeclarationKind::Const
                ) =>
            {
                let mut scope = FxHashMap::default();
                for item in &declaration.declarations {
                    self.map_pattern_lexicals(&item.id, &mut scope);
                }
                self.push_lexical_bindings(scope);
                true
            }
            _ => false,
        };
        self.for_initializer(item.init.as_ref());
        let head = self.code.len() as u32;
        let condition_end = item.test.as_ref().map(|test| self.condition(test));
        self.push_control(ControlKind::Loop, label);
        self.statement(&item.body);
        let control = self.controls.pop().unwrap();
        let update = self.code.len() as u32;
        self.patch_edges(&control.continues, update);
        if let Some(expression) = &item.update {
            self.expression(expression);
        }
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        if let Some(edge) = condition_end {
            self.patch_to(edge, end);
        }
        self.patch_edges(&control.breaks, end);
        if scoped {
            self.lexical_scopes.pop();
        }
    }

    fn for_initializer(&mut self, init: Option<&ForStatementInit<'_>>) {
        let Some(init) = init else { return };
        if let ForStatementInit::VariableDeclaration(value) = init {
            self.variables(value);
        } else if let Some(value) = init.as_expression() {
            self.expression(value);
        }
    }

    fn switch_statement(&mut self, item: &SwitchStatement<'_>) {
        let discriminant = self.expression(&item.discriminant);
        self.push_switch_lexical_scope(&item.cases);
        for case in &item.cases {
            self.emit_hoisted(&case.consequent);
        }
        let mut case_edges = Vec::with_capacity(item.cases.len());
        for case in &item.cases {
            case_edges.push(case.test.as_ref().map(|test| {
                let test = self.expression(test);
                let matched = self.emit_binary(
                    STRICT_EQUAL_OPERATOR,
                    Operand::register(discriminant),
                    Operand::register(test),
                );
                let next = self.emit(Op::JumpFalse, matched, 0, 0, 0);
                let selected = self.emit(Op::Jump, 0, 0, 0, 0);
                self.patch(next);
                selected
            }));
        }
        let no_match = self.emit(Op::Jump, 0, 0, 0, 0);
        self.push_control(ControlKind::Switch, None);
        let mut targets = Vec::with_capacity(item.cases.len());
        for case in &item.cases {
            targets.push(self.code.len() as u32);
            self.statements(&case.consequent);
        }
        let control = self.controls.pop().unwrap();
        let end = self.code.len() as u32;
        for (edge, target) in case_edges.into_iter().zip(&targets) {
            if let Some(edge) = edge {
                self.patch_to(edge, *target);
            }
        }
        let fallback = item
            .cases
            .iter()
            .position(|case| case.test.is_none())
            .map_or(end, |index| targets[index]);
        self.patch_to(no_match, fallback);
        self.patch_edges(&control.breaks, end);
        self.pop_lexical_scope();
    }

    fn break_statement(&mut self, item: &BreakStatement<'_>) {
        let index = if let Some(label) = &item.label {
            let label = self.owner.atom(label.name.as_str());
            self.controls
                .iter()
                .rposition(|control| control.label == Some(label))
        } else {
            self.controls.iter().rposition(|control| {
                matches!(control.kind, ControlKind::Loop | ControlKind::Switch)
            })
        };
        let Some(index) = index else {
            self.owner.reject(
                item.span,
                if item.label.is_some() {
                    "break label is not in scope"
                } else {
                    "break outside loop or switch"
                },
            );
            return;
        };
        let edge = self.emit(Op::Jump, 0, 0, 0, 0);
        if let Some(context) = self.finally_contexts.last_mut() {
            context.abrupt_edges.push(FinallyAbrupt {
                edge,
                control: index,
                continue_edge: false,
            });
        } else {
            self.controls[index].breaks.push(edge);
        }
    }

    fn continue_statement(&mut self, item: &ContinueStatement<'_>) {
        let index = if let Some(label) = &item.label {
            let label = self.owner.atom(label.name.as_str());
            self.controls.iter().rposition(|control| {
                control.kind == ControlKind::Loop && control.label == Some(label)
            })
        } else {
            self.controls
                .iter()
                .rposition(|control| control.kind == ControlKind::Loop)
        };
        let Some(index) = index else {
            self.owner.reject(
                item.span,
                if item.label.is_some() {
                    "continue label does not target a loop"
                } else {
                    "continue outside loop"
                },
            );
            return;
        };
        let edge = self.emit(Op::Jump, 0, 0, 0, 0);
        if let Some(context) = self.finally_contexts.last_mut() {
            context.abrupt_edges.push(FinallyAbrupt {
                edge,
                control: index,
                continue_edge: true,
            });
        } else {
            self.controls[index].continues.push(edge);
        }
    }

    fn labeled_statement(&mut self, item: &LabeledStatement<'_>) {
        let label = self.owner.atom(item.label.name.as_str());
        match &item.body {
            Statement::WhileStatement(body) => {
                self.while_statement_labeled(body, Some(label));
                return;
            }
            Statement::DoWhileStatement(body) => {
                self.do_while_statement_labeled(body, Some(label));
                return;
            }
            Statement::ForStatement(body) => {
                self.for_statement_labeled(body, Some(label));
                return;
            }
            Statement::ForInStatement(body) => {
                self.for_in_statement_labeled(body, Some(label));
                return;
            }
            Statement::ForOfStatement(body) => {
                self.for_of_statement_labeled(body, Some(label));
                return;
            }
            _ => {}
        }
        self.push_control(ControlKind::Label, Some(label));
        self.statement(&item.body);
        let control = self.controls.pop().unwrap();
        let end = self.code.len() as u32;
        self.patch_edges(&control.breaks, end);
    }

    pub(super) fn push_control(&mut self, kind: ControlKind, label: Option<Atom>) {
        self.controls.push(ControlTarget {
            kind,
            label,
            breaks: vec![],
            continues: vec![],
        });
    }

    pub(super) fn patch_edges(&mut self, edges: &[usize], target: u32) {
        for edge in edges {
            self.patch_to(*edge, target);
        }
    }

    pub(super) fn patch_to(&mut self, edge: usize, target: u32) {
        self.patch_instruction(edge, target);
    }
}
