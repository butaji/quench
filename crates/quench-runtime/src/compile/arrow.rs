use super::*;

impl Compiler<'_> {
    pub(super) fn compile_arrow_function(
        &mut self,
        value: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        context: &super::replay::ArrowContext,
    ) -> u32 {
        let super::replay::ArrowContext {
            ref scopes,
            parent,
            super_static,
            super_home,
            super_home_atom,
            inherited_strict,
            class_initializer,
            lexical_this_atom,
            super_call_binds_this,
            with_depth,
        } = *context;
        if let Some(id) = self.skip_replayed_child() {
            return id;
        }
        let id = self.functions.reserve();
        let source_span = self.source_span(value.span);
        let body_lexicals = if let oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) = &value.body
        {
            self.collect_body_lexical_bindings(&body.statements)
        } else {
            Vec::new()
        };
        let lexical_atoms: Vec<_> = body_lexicals.iter().map(|(atom, _)| *atom).collect();
        let params = FunctionCompiler::params_from_formals(&value.params, self);
        let params: Vec<Atom> = params.iter().map(|name| self.atom(name)).collect();
        let mut locals = params.clone();
        for name in FunctionCompiler::parameter_bound_names(&value.params) {
            let atom = self.atom(&name);
            if !locals.contains(&atom) {
                locals.push(atom);
            }
        }
        let parameter_atoms = FunctionCompiler::parameter_bound_names(&value.params)
            .iter()
            .map(|name| self.atom(name))
            .collect();
        let parameter_local_count = locals.len();
        let strict = inherited_strict
            || matches!(&value.body, oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) if body.directives.iter().any(|directive| directive.directive == "use strict"));
        let function_scope = match &value.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                self.collect_locals(&body.statements, &mut locals, strict, false)
            }
            _ => locals.iter().copied().collect(),
        };
        let annex_b_collisions = match &value.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                early::annex_b_lexical_collisions(&body.statements)
            }
            _ => FxHashSet::default(),
        };
        let environment_atoms = locals
            .iter()
            .copied()
            .filter(|atom| function_scope.contains(atom))
            .collect();
        let dynamic_eval = early::parameters_contain_direct_eval(&value.params)
            || match &value.body {
                oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                    early::body_contains_direct_eval(&body.statements)
                }
                body => early::expression_contains_direct_eval(
                    body.as_expression().expect("arrow expression body"),
                ),
            };
        let simple_parameters = !FunctionCompiler::has_non_simple_parameters(&value.params);
        let (body_statements, expression_body) = match &value.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => (&body.statements[..], None),
            body => (&[][..], body.as_expression()),
        };
        // An arrow inside a field initializer or static block (`class_initializer`) still
        // owns its parameters and locals: the initializer's restrictions (no `arguments`) are
        // early errors, and direct eval is excluded below.
        // As a function's (`direct_parameters`): an identifier rest parameter's local is
        // filled with the rest array as the arrow activates, so it promotes as any parameter.
        // A list binding its names apart promotes just its hidden argument slots, as a
        // function's does (`Compiler::compile_function`).
        let direct_parameters = FunctionCompiler::binds_parameters_directly(&value.params);
        let hidden_parameters =
            !direct_parameters && FunctionCompiler::arguments_promotable_apart(&value.params);
        let super::PromotedLocalPlan {
            promoted_local_slots,
            clobbered_local_slots,
            ..
        } = self.promoted_local_plan(
            parent.is_some()
                && (direct_parameters || hidden_parameters)
                && !value.r#async
                && with_depth == 0
                && !dynamic_eval,
            hidden_parameters.then_some(value.params.items.len()),
            None,
            body_statements,
            expression_body,
            &locals,
            &function_scope,
            &lexical_atoms,
            None,
        );
        let binds_dynamically = dynamic_eval
            || match &value.body {
                oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                    early::body_may_bind_dynamically(&body.statements, None)
                }
                body => early::body_may_bind_dynamically(&[], body.as_expression()),
            };
        self.dynamic_binding_scopes += u32::from(binds_dynamically);
        let mut function = FunctionCompiler::new(
            self,
            locals,
            function_scope,
            annex_b_collisions,
            scopes.clone(),
            id,
            (false, false),
            value.r#async,
            false,
            None,
            parameter_local_count,
            with_depth,
            &promoted_local_slots,
            &clobbered_local_slots,
        );
        function.strict = strict;
        function.super_static = super_static;
        function.super_home = super_home;
        function.super_home_atom = super_home_atom;
        function.class_initializer = class_initializer;
        function.super_call_binds_this = super_call_binds_this;
        function.this_override = lexical_this_atom.map(|atom| function.load_atom(atom));
        function.dynamic_eval = dynamic_eval;
        let lexical_slots = lexical_atoms
            .iter()
            .filter_map(|atom| function.local_slots.get(atom).copied())
            .collect();
        function.initialize_tdz_slots(lexical_slots);
        function.emit_parameter_bindings(&value.params);
        function.push_body_lexical_bindings(&body_lexicals);
        match &value.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                function.emit_hoisted(&body.statements);
                function.function_body_statements(&body.statements);
                let undefined = function.literal(Constant::Undefined);
                function.emit(Op::Return, undefined, 0, 0, 0);
            }
            body => {
                let expression = body.as_expression().expect("arrow expression body");
                let result = function.expression(expression);
                function.emit(Op::Return, result, 0, 0, 0);
            }
        }
        // As in `Compiler::compile_function`: no doubling slack beside the syntax tree.
        function.code.shrink_to_fit();
        function.source_positions.shrink_to_fit();
        let name_bindings = function.name_bindings();
        let undefined_register = function.used_undefined_register();
        let environment_clones = function.per_site_environment_clones();
        let result = BcFunction {
            parent,
            name: None,
            inferred_name: None,
            is_arrow: true,
            self_binding_slot: None,
            source_span,
            params: params.len() as u16,
            length: value
                .params
                .items
                .iter()
                .take_while(|item| item.initializer.is_none())
                .count() as u16,
            parameter_end_pc: 0,
            parameter_atoms,
            rest: value.params.rest.is_some(),
            is_async: value.r#async,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom,
            constructible: false,
            class_initializer,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false,
            prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters,
            strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone().into(),
            environment_atoms,
            selective_capture_slots: None,
            local_registers: function.promoted_local_layout().into(),
            inherited_with_scope: with_depth != 0,
            lexical_atoms: lexical_atoms.into(),
            global_declarations: None,
            name_bindings,
            binding_sites: function.binding_sites.into(),
            source_positions: function.source_positions.into(),
            environment_clones: environment_clones.into(),
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            declared_atom_sets: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            dynamic_scope_access: Default::default(),
            js_tier_candidate: Default::default(),
            call_plan: Default::default(),
            handlers: function.handlers.into(),
            register_roots: Default::default(),
            undefined_register,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        self.functions.set(id, result);
        self.dynamic_binding_scopes -= u32::from(binds_dynamically);
        self.record_function_exit(id);
        id
    }
}
