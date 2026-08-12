impl StatementReducer {
    pub(super) fn new_with_global(source_type: SourceType, global: bool) -> Self {
        let locals = initialize_statement_locals(source_type);
        let (mut ops, next_register) = initialize_statement_ops(global);
        ops.push(Op::StoreLocal {
            slot: 0,
            src: next_register,
        });
        Self {
            locals,
            ops,
            next_slot: 1,
            next_register: next_register.saturating_add(1),
            script: source_type.is_script(),
        }
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
            .extend(crate::semantic_early::lexically_declared_names_in(statements));
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
        );
        if global_script {
            instantiate_global_script_functions(statements, facts, self)?;
        }
        self.next_register = self
            .next_register
            .max(crate::reduce_support::register_base(&self.locals));
        append_scoped_statements(self, statements, facts, program_scope, global_script)
    }
}

fn initialize_statement_locals(source_type: SourceType) -> HashMap<String, u16> {
    let mut locals = HashMap::from([(GLOBAL_THIS.to_string(), 0)]);
    if !source_type.is_module() {
        locals.insert(SCRIPT_THIS_SLOT.to_string(), 0);
    }
    locals
}

fn initialize_statement_ops(global: bool) -> (Vec<Op>, u16) {
    if global {
        return (vec![Op::LoadCurrentGlobal { dst: 0 }], 0);
    }
    let mut ops = Vec::new();
    let mut next_register = 0;
    let properties = crate::globals::script_properties(&mut ops, &mut next_register);
    let object = next_register;
    ops.push(Op::MakeObject {
        dst: object,
        properties: properties.clone(),
    });
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
        if global_script && matches!(statement, Statement::FunctionDeclaration(_)) {
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
