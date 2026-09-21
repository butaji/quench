use super::*;

impl Compiler<'_> {
    pub(super) fn compile_arrow_function(
        &mut self,
        value: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
    ) -> u32 {
        let id = self.functions.len() as u32;
        self.functions.push(None);
        let params = FunctionCompiler::params_from_formals(&value.params, self);
        let params: Vec<Atom> = params.iter().map(|name| self.atom(name)).collect();
        let mut locals = params.clone();
        if let oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) = &value.body {
            self.collect_locals(&body.statements, &mut locals);
        }
        let mut function = FunctionCompiler::new(self, locals, scopes.to_vec(), id);
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
        }
        let result = BcFunction {
            parent,
            name: None,
            params: params.len() as u16,
            locals: function.locals.len() as u16,
            code: function.code,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            handlers: function.handlers,
            register_root_offset: u32::MAX,
        };
        self.functions[id as usize] = Some(result);
        id
    }
}
