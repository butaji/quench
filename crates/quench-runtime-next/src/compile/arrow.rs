use super::*;

impl Compiler<'_> {
    pub(super) fn compile_arrow_function(
        &mut self,
        value: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        super_static: bool,
        super_home: bool,
        super_home_atom: Option<Atom>,
        inherited_strict: bool,
        class_field_initializer: bool,
        lexical_this_atom: Option<Atom>,
        super_call_binds_this: bool,
        with_depth: u16,
    ) -> u32 {
        let id = self.functions.len() as u32;
        self.functions.push(None);
        let lexical_atoms = if let oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) = &value.body
        {
            self.collect_lexical_atoms(&body.statements)
        } else {
            Vec::new()
        };
        let arrow_marker = self.atom("\0rqj:arrow");
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
                self.collect_locals(&body.statements, &mut locals, strict)
            }
            _ => locals.iter().copied().collect(),
        };
        let mut function = FunctionCompiler::new(
            self,
            locals,
            function_scope,
            scopes.to_vec(),
            id,
            (false, false),
            value.r#async,
            false,
            false,
            None,
            parameter_local_count,
            with_depth,
        );
        function.strict = strict;
        function.super_static = super_static;
        function.super_home = super_home;
        function.super_home_atom = super_home_atom;
        function.class_field_initializer = class_field_initializer;
        function.super_call_binds_this = super_call_binds_this;
        function.this_override = lexical_this_atom.map(|atom| function.load_atom(atom));
        function.dynamic_eval = early::parameters_contain_direct_eval(&value.params);
        function.emit_parameter_bindings(&value.params);
        match &value.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => {
                function.emit_hoisted(&body.statements);
                function.statements(&body.statements);
                let undefined = function.literal(Constant::Undefined);
                function.emit(Op::Return, undefined, 0, 0, 0);
            }
            body => {
                let expression = body.as_expression().expect("arrow expression body");
                let result = function.expression(expression);
                function.emit(Op::Return, result, 0, 0, 0);
            }
        }
        let captures_locals = function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::MakeClosure)
            || function
                .wide
                .iter()
                .any(|instruction| instruction.op() == Op::MakeClosure);
        if captures_locals {
            for instruction in &mut function.code {
                let op = match instruction.op() {
                    Op::LoadLocal => Op::LoadEnvLocal,
                    Op::StoreLocal => Op::StoreEnvLocal,
                    other => other,
                };
                instruction.set_op(op);
            }
            for instruction in &mut function.wide {
                let op = match instruction.op() {
                    Op::LoadLocal => Op::LoadEnvLocal,
                    Op::StoreLocal => Op::StoreEnvLocal,
                    other => other,
                };
                instruction.set_op(op);
            }
        }
        let result = BcFunction {
            parent,
            // Keep the arrow/non-constructor invariant in the residual
            // function table without adding a second callable representation.
            // The marker is VM-internal and never materialized as `.name`.
            name: Some(arrow_marker),
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
            super_home_atom,
            constructible: false,
            class_field_initializer,
            parameter_eval_arguments_error: false,
            arguments_slot: None,
            strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone(),
            lexical_atoms,
            global_lexical_atoms: Vec::new(),
            global_var_atoms: Vec::new(),
            global_function_atoms: Vec::new(),
            global_immutable_atoms: Vec::new(),
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            handlers: function.handlers,
            register_root_offset: u32::MAX,
        };
        self.functions[id as usize] = Some(result);
        id
    }
}
