use super::*;

impl Compiler<'_> {
    pub(super) fn compile_arrow_function(
        &mut self,
        value: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
    ) -> u32 {
        let inherited_strict = parent
            .and_then(|id| self.functions.get(id as usize).and_then(Option::as_ref))
            .is_some_and(|function| function.strict);
        let id = self.functions.len() as u32;
        self.functions.push(None);
        let arrow_marker = self.atom("\0rqj:arrow");
        let params = FunctionCompiler::params_from_formals(&value.params, self);
        let params: Vec<Atom> = params.iter().map(|name| self.atom(name)).collect();
        let mut locals = params.clone();
        if let oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) = &value.body {
            self.collect_locals(&body.statements, &mut locals);
        }
        let mut function = FunctionCompiler::new(
            self,
            locals,
            scopes.to_vec(),
            id,
            (false, false),
            value.r#async,
            false,
        );
        let strict = inherited_strict
            || matches!(&value.body, oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) if body.directives.iter().any(|directive| directive.directive == "use strict"));
        function.strict = strict;
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
            rest: value.params.rest.is_some(),
            is_async: value.r#async,
            is_generator: false,
            arguments_slot: None,
            strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone(),
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
