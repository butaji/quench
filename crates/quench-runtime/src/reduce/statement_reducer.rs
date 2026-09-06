impl StatementReducer {
    pub(super) fn set_source_type(&mut self, source_type: SourceType) {
        self.script = source_type.is_script();
        if source_type.is_module() {
            self.locals.remove(SCRIPT_THIS_SLOT);
            self.locals.insert(MODULE_THIS_SLOT.to_string(), 0);
            let slot = self.next_slot;
            self.next_slot = self.next_slot.saturating_add(1);
            self.locals
                .insert(super::reduce_statements::IMPORT_META_SLOT.to_string(), slot);
            let object = self.next_register;
            self.next_register = self.next_register.saturating_add(1);
            self.ops.push(Op::MakeObject {
                dst: object,
                properties: Vec::new(),
            });
            let prototype = self.next_register;
            self.next_register = self.next_register.saturating_add(1);
            self.ops.push(Op::Const {
                dst: prototype,
                value: crate::ops::Constant::Null,
            });
            self.ops.push(Op::SetPrototype { object, prototype });
            self.ops.push(Op::StoreLocal { slot, src: object });
        }
    }

    pub(super) fn local_slots(&self) -> HashMap<String, u16> {
        self.locals.clone()
    }

    pub(super) const fn frame_register_count(&self) -> u16 {
        self.next_register
    }

    pub(super) fn new_with_global(source_type: SourceType, global: bool) -> Self {
        Self::new_with_modes(source_type, global, global)
    }

    pub(super) fn new_with_script_this_global(source_type: SourceType) -> Self {
        Self::new_with_modes(source_type, false, true)
    }

    fn new_with_modes(source_type: SourceType, global: bool, script_this_global: bool) -> Self {
        let locals = initialize_statement_locals(source_type);
        let (mut ops, next_register) = initialize_statement_ops(global, script_this_global);
        ops.push(Op::StoreLocal {
            slot: 0,
            src: next_register,
        });
        let mut state = Self {
            locals,
            ops,
            next_slot: 1,
            next_register: next_register.saturating_add(1),
            script: source_type.is_script(),
        };
        if source_type.is_module() {
            state.set_source_type(source_type);
        }
        state
    }

    pub(super) fn append(
        &mut self,
        statements: &[Statement<'_>],
        facts: &mut ProgramDb,
        program_scope: bool,
    ) -> Result<Option<u16>, Vec<String>> {
        let barrier_len = facts.eval_var_barrier.len();
        facts
            .eval_var_barrier
            .extend(crate::semantic_early::lexically_declared_names_in(
                statements,
            ));
        let result = self.append_scoped(statements, facts, program_scope);
        facts.eval_var_barrier.truncate(barrier_len);
        result
    }

    fn append_scoped(
        &mut self,
        statements: &[Statement<'_>],
        facts: &mut ProgramDb,
        program_scope: bool,
    ) -> Result<Option<u16>, Vec<String>> {
        let global_script = self.script && program_scope;
        crate::reduce_support::instantiate_script_declarations(
            statements,
            &mut self.locals,
            &mut self.next_slot,
            &mut self.ops,
            global_script,
            facts.strict,
        );
        if !self.script {
            crate::reduce_support::predeclare_lexicals(
                statements,
                &mut self.locals,
                &mut self.next_slot,
            );
            crate::reduce_support::predeclare_module_vars(
                statements,
                &mut self.locals,
                &mut self.next_slot,
            );
            emit_module_lexical_tdz(&mut self.ops, statements, &self.locals);
        }
        if global_script {
            instantiate_global_script_functions(statements, facts, self)?;
            // Global `var` declarations are instantiated before any script
            // statement, even when their source declaration appears later.
            // Emit their object-binding mechanics in the prefix rather than
            // at the source position so early `delete`/reads see the binding.
            for statement in statements {
                if matches!(
                    statement,
                    Statement::VariableDeclaration(declaration)
                        if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var
                ) {
                    crate::reduce_support::mirror_script_bindings(
                        statement,
                        &self.locals,
                        &mut self.ops,
                        &mut self.next_register,
                    );
                }
            }
        } else if !self.script {
            if program_scope {
                for statement in statements {
                    if matches!(
                        statement,
                        Statement::VariableDeclaration(declaration)
                            if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var
                    ) {
                        crate::reduce_support::mirror_script_bindings(
                            statement,
                            &self.locals,
                            &mut self.ops,
                            &mut self.next_register,
                        );
                    }
                }
            }
            instantiate_module_functions(statements, facts, self)?;
        }
        self.next_register = self
            .next_register
            .max(crate::reduce_support::register_base(&self.locals));
        let stack = crate::using_scope::reserve(statements, &mut self.locals, &mut self.next_slot);
        crate::using_scope::emit_tdz(statements, &mut self.ops, &self.locals);
        let await_using = crate::using_scope::has_await_using(statements);
        if let Some(stack) = stack {
            crate::using_scope::emit_create(
                &mut self.ops,
                stack,
                await_using,
                &mut self.next_register,
            );
        }
        let body_start = self.ops.len();
        let last = append_scoped_statements(self, statements, facts, program_scope, global_script)?;
        wrap_program_using(self, stack, await_using, body_start)?;
        Ok(last)
    }
}

fn wrap_program_using(
    state: &mut StatementReducer,
    stack: Option<u16>,
    await_using: bool,
    body_start: usize,
) -> Result<(), Vec<String>> {
    let Some(stack) = stack else {
        return Ok(());
    };
    let body = state.ops.split_off(body_start);
    state.ops.extend(crate::using_scope::wrap(
        body,
        stack,
        await_using,
        &mut state.next_register,
    )?);
    Ok(())
}

fn instantiate_module_functions(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    state: &mut StatementReducer,
) -> Result<(), Vec<String>> {
    for statement in statements {
        let function = match statement {
            Statement::FunctionDeclaration(function) => Some(function),
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(oxc::ast::ast::Declaration::FunctionDeclaration(function)) => Some(function),
                _ => None,
            },
            _ => None,
        };
        if let Some(function) = function {
            crate::reduce::reduce_function_declaration(
                function,
                &mut state.ops,
                facts,
                &mut state.next_register,
                &mut state.next_slot,
                &mut state.locals,
            )?;
        }
    }
    Ok(())
}

fn initialize_statement_locals(source_type: SourceType) -> HashMap<String, u16> {
    let mut locals = HashMap::from([(GLOBAL_THIS.to_string(), 0)]);
    if source_type.is_module() {
        locals.insert(super::reduce_statements::MODULE_THIS_SLOT.to_string(), 0);
    } else {
        locals.insert(SCRIPT_THIS_SLOT.to_string(), 0);
    }
    locals
}

fn initialize_statement_ops(global: bool, script_this_global: bool) -> (Vec<Op>, u16) {
    if global {
        return (vec![Op::LoadCurrentGlobal { dst: 0 }], 0);
    }
    let mut ops = Vec::new();
    let mut next_register = 0;
    let properties = crate::globals::script_properties(&mut ops, &mut next_register);
    let object = next_register;
    if script_this_global {
        ops.push(Op::MakeGlobalObjectView {
            dst: object,
            properties: properties.clone(),
        });
    } else {
        ops.push(Op::MakeObject {
            dst: object,
            properties: properties
                .iter()
                .map(|(name, value)| (name.clone().into(), *value))
                .collect(),
        });
    }
    next_register = next_register.saturating_add(1);
    append_script_properties_ops(&mut ops, object, &properties, &mut next_register);
    (ops, object)
}

fn append_script_properties_ops(
    ops: &mut Vec<Op>,
    object: u16,
    properties: &[(String, u16)],
    next_register: &mut u16,
) {
    for (name, src) in properties {
        let key = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::Const {
            dst: key,
            value: crate::ops::Constant::String(name.clone()),
        });
        ops.push(Op::DefineProperty {
            object,
            key,
            value: *src,
            kind: PropertyDefinitionKind::Data,
            enumerable: false,
        });
    }
}

fn instantiate_global_script_functions(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    state: &mut StatementReducer,
) -> Result<(), Vec<String>> {
    super::reduce_eval::instantiate_functions(
        statements,
        facts,
        (
            &mut state.ops,
            &mut state.next_register,
            &mut state.next_slot,
            &mut state.locals,
        ),
        crate::reduce_support::EvalBehavior::Script,
    )?;
    Ok(())
}

fn append_scoped_statements(
    state: &mut StatementReducer,
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    program_scope: bool,
    global_script: bool,
) -> Result<Option<u16>, Vec<String>> {
    let mut last = None;
    for statement in statements {
        if (global_script && matches!(statement, Statement::FunctionDeclaration(_)))
            || (!global_script && is_module_function_declaration(statement))
        {
            continue;
        }
        let limit = program_scope
            .then(|| crate::reduce_support::script_lexical_slot(statement, &state.locals))
            .flatten()
            .map(|slot| std::mem::replace(&mut state.next_slot, slot));
        let next = reduce_state_statement(state, statement, facts, program_scope)?.or(last);
        state.next_slot = limit.map_or(state.next_slot, |limit| limit.max(state.next_slot));
        last = next;
    }
    Ok(last)
}

fn is_module_function_declaration(statement: &Statement<'_>) -> bool {
    if matches!(statement, Statement::FunctionDeclaration(_)) {
        return true;
    }
    let Statement::ExportNamedDeclaration(export) = statement else {
        return false;
    };
    matches!(
        &export.declaration,
        Some(oxc::ast::ast::Declaration::FunctionDeclaration(_))
    )
}

fn emit_module_lexical_tdz(
    ops: &mut Vec<Op>,
    statements: &[Statement<'_>],
    locals: &HashMap<String, u16>,
) {
    for statement in statements {
        for name in crate::reduce_support::lexical_bound_names(statement) {
            if let Some(&slot) = locals.get(&name) {
                ops.push(Op::MarkUninitialized { slot, shared: true });
                if crate::reduce_support::lexical_declaration(statement).is_some_and(
                    |declaration| {
                        matches!(
                            declaration.kind,
                            oxc::ast::ast::VariableDeclarationKind::Const
                                | oxc::ast::ast::VariableDeclarationKind::Using
                                | oxc::ast::ast::VariableDeclarationKind::AwaitUsing
                        )
                    },
                ) {
                    ops.push(Op::MarkImmutable { slot });
                }
            }
        }
    }
}
