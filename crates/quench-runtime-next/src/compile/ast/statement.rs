use super::*;

const STRICT_EQUAL_OPERATOR: u32 = 2;

impl FunctionCompiler<'_, '_> {
    pub(crate) fn statements(&mut self, body: &[Statement<'_>]) {
        for statement in body {
            self.statement(statement);
            self.release_temporaries();
        }
    }

    fn statement(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::EmptyStatement(_) | Statement::FunctionDeclaration(_) => {}
            Statement::ClassDeclaration(item) => {
                self.class_declaration(item);
            }
            Statement::ExpressionStatement(item) => {
                self.expression(&item.expression);
            }
            Statement::BlockStatement(block) => self.statements(&block.body),
            Statement::VariableDeclaration(item) => self.variables(item),
            Statement::ReturnStatement(item) => self.return_statement(item),
            Statement::IfStatement(item) => self.if_statement(item),
            Statement::WhileStatement(item) => self.while_statement(item),
            Statement::DoWhileStatement(item) => self.do_while_statement(item),
            Statement::ForStatement(item) => self.for_statement(item),
            Statement::ForOfStatement(item) => self.for_of_statement(item),
            Statement::SwitchStatement(item) => self.switch_statement(item),
            Statement::BreakStatement(item) => self.break_statement(item),
            Statement::ContinueStatement(item) => self.continue_statement(item),
            Statement::LabeledStatement(item) => self
                .owner
                .reject(item.span, "labeled statements are unsupported"),
            Statement::ThrowStatement(item) => {
                let value = self.expression(&item.argument);
                self.emit(Op::Throw, value, 0, 0, 0);
            }
            Statement::TryStatement(item) => self.try_statement(item),
            _ => self.owner.reject(
                statement.span(),
                "statement is outside the supported subset",
            ),
        }
    }

    fn return_statement(&mut self, item: &ReturnStatement<'_>) {
        if let Some(value) = &item.argument {
            self.return_expression(value);
        } else {
            let value = self.literal(Constant::Undefined);
            self.emit(Op::Return, value, 0, 0, 0);
        }
    }

    fn return_expression(&mut self, expression: &Expression<'_>) {
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
                self.emit(Op::Return, value, 0, 0, 0);
            }
        }
    }

    fn return_logical(&mut self, value: &LogicalExpression<'_>) {
        let left = self.expression(&value.left);
        let false_edge = self.emit(Op::JumpFalse, left, 0, 0, 0);
        if value.operator.is_or() {
            self.emit(Op::Return, left, 0, 0, 0);
            self.patch(false_edge);
            self.return_expression(&value.right);
        } else {
            self.return_expression(&value.right);
            self.patch(false_edge);
            self.emit(Op::Return, left, 0, 0, 0);
        }
    }

    fn variables(&mut self, declaration: &VariableDeclaration<'_>) {
        for item in &declaration.declarations {
            if let Some(init) = &item.init {
                let value = self.expression(init);
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
        let head = self.code.len() as u32;
        let condition_end = self.condition(&item.test);
        self.push_control(ControlKind::Loop);
        self.statement(&item.body);
        let control = self.controls.pop().unwrap();
        self.patch_edges(&control.continues, head);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(condition_end, end);
        self.patch_edges(&control.breaks, end);
    }

    fn do_while_statement(&mut self, item: &DoWhileStatement<'_>) {
        let head = self.code.len() as u32;
        self.push_control(ControlKind::Loop);
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
        self.for_initializer(item.init.as_ref());
        let head = self.code.len() as u32;
        let condition_end = item.test.as_ref().map(|test| self.condition(test));
        self.push_control(ControlKind::Loop);
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
    }

    fn for_of_statement(&mut self, item: &ForOfStatement<'_>) {
        if item.r#await {
            self.owner
                .reject(item.span, "for-await-of is outside the supported subset");
            return;
        }
        let iterator_atom = self.hidden_local("\0rqj:for-of:iterator");
        let source_value = self.expression(&item.right);
        let iterator = self.reg();
        self.emit(Op::GetIterator, iterator, source_value, 0, 0);
        self.store_atom(iterator_atom, iterator);
        let head = self.code.len() as u32;
        let iterator = self.load_atom(iterator_atom);
        let next_atom = self.owner.atom("next");
        let next_cache = self.owner.cache_site();
        let method_site = self.owner.method_sites.len() as u32;
        self.owner
            .method_sites
            .push((next_atom, next_cache, Vec::new(), None));
        let result = self.reg();
        self.emit(Op::CallMethod, result, iterator, 0, method_site);
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
        self.bind_for_of_left(&item.left, value);
        self.push_control(ControlKind::Loop);
        self.statement(&item.body);
        let control = self.controls.pop().unwrap();
        let update = self.code.len() as u32;
        self.patch_edges(&control.continues, update);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
        self.patch_edges(&control.breaks, end);
    }

    fn bind_for_of_left(&mut self, left: &ForStatementLeft<'_>, value: Register) {
        match left {
            ForStatementLeft::VariableDeclaration(declaration)
                if declaration.declarations.len() == 1 =>
            {
                self.bind_pattern(&declaration.declarations[0].id, value);
            }
            _ => {
                if let Some(target) = left.as_simple_assignment_target() {
                    self.assign_target(target, value, 0);
                } else {
                    self.owner
                        .reject(left.span(), "for-of assignment target unsupported");
                }
            }
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
        self.push_control(ControlKind::Switch);
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
    }

    fn break_statement(&mut self, item: &BreakStatement<'_>) {
        if item.label.is_some() {
            self.owner.reject(item.span, "labeled break is unsupported");
            return;
        }
        let edge = self.emit(Op::Jump, 0, 0, 0, 0);
        if let Some(control) = self.controls.last_mut() {
            control.breaks.push(edge);
        } else {
            self.owner.reject(item.span, "break outside loop or switch");
        }
    }

    fn continue_statement(&mut self, item: &ContinueStatement<'_>) {
        if item.label.is_some() {
            self.owner
                .reject(item.span, "labeled continue is unsupported");
            return;
        }
        let Some(index) = self
            .controls
            .iter()
            .rposition(|control| control.kind == ControlKind::Loop)
        else {
            self.owner.reject(item.span, "continue outside loop");
            return;
        };
        let edge = self.emit(Op::Jump, 0, 0, 0, 0);
        self.controls[index].continues.push(edge);
    }

    fn push_control(&mut self, kind: ControlKind) {
        self.controls.push(ControlTarget {
            kind,
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

    fn try_statement(&mut self, item: &TryStatement<'_>) {
        if item.finalizer.is_some() {
            self.owner
                .reject(item.span, "try/finally is outside the supported subset");
            return;
        }
        let Some(handler) = &item.handler else {
            self.owner
                .reject(item.span, "try without catch is unsupported");
            return;
        };
        let slot = self.catch_slot(handler);
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
        self.statements(&handler.body.body);
        self.patch(skip);
    }

    fn catch_slot(&mut self, handler: &CatchClause<'_>) -> Option<u16> {
        match &handler.param {
            Some(parameter)
                if matches!(&parameter.pattern, BindingPattern::BindingIdentifier(_)) =>
            {
                let BindingPattern::BindingIdentifier(id) = &parameter.pattern else {
                    unreachable!()
                };
                let atom = self.owner.atom(id.name.as_str());
                self.locals
                    .iter()
                    .position(|value| *value == atom)
                    .map(|value| value as u16)
            }
            None => None,
            _ => {
                self.owner
                    .reject(handler.span, "catch binding pattern is unsupported");
                None
            }
        }
    }
}
