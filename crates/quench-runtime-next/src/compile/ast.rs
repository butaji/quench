use super::*;

mod call;
mod expression;
mod optional;
mod statement;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlKind {
    Loop,
    Switch,
}

enum UpdateTarget {
    Name(Atom),
    Field(Atom, Register),
    Index(Register, Register),
}

struct ControlTarget {
    kind: ControlKind,
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

pub(super) struct FunctionCompiler<'a, 'b> {
    pub(super) owner: &'a mut Compiler<'b>,
    pub(super) locals: Vec<Atom>,
    pub(super) code: Vec<Instr>,
    next_reg: Register,
    pub(super) max_reg: Register,
    pub(super) local_slots: Rc<FxHashMap<Atom, u16>>,
    pub(super) scopes: Vec<Rc<FxHashMap<Atom, u16>>>,
    pub(super) function_id: u32,
    pub(super) handlers: Vec<crate::bytecode::Handler>,
    controls: Vec<ControlTarget>,
    packed_domain_error: bool,
}

impl<'a, 'b> FunctionCompiler<'a, 'b> {
    pub(super) fn new(
        owner: &'a mut Compiler<'b>,
        locals: Vec<Atom>,
        scopes: Vec<Rc<FxHashMap<Atom, u16>>>,
        function_id: u32,
    ) -> Self {
        if locals.len() > usize::from(u16::MAX) {
            owner.reject(Span::default(), "function exceeds the local-slot limit");
        }
        let local_slots = Rc::new(
            locals
                .iter()
                .enumerate()
                .map(|(slot, atom)| (*atom, slot as u16))
                .collect(),
        );
        Self {
            owner,
            locals,
            code: vec![],
            next_reg: 0,
            max_reg: 0,
            local_slots,
            scopes,
            function_id,
            handlers: vec![],
            controls: vec![],
            packed_domain_error: false,
        }
    }

    pub(super) fn reg(&mut self) -> Register {
        let value = self.next_reg;
        if value > Instr::MAX_PAYLOAD {
            self.reject_packed_domain();
            return 0;
        }
        self.next_reg += 1;
        self.max_reg = self.max_reg.max(self.next_reg);
        value
    }

    pub(super) fn emit(
        &mut self,
        op: Op,
        a: Register,
        b: Register,
        c: Register,
        imm: u32,
    ) -> usize {
        let instruction = Instr::try_new(op, a, b, c, imm).unwrap_or_else(|| {
            self.reject_packed_domain();
            Instr::new(Op::Nop, 0, 0, 0, 0)
        });
        self.code.push(instruction);
        self.code.len() - 1
    }

    fn patch(&mut self, at: usize) {
        let target = self.code.len() as u32;
        self.patch_instruction(at, target);
    }

    pub(super) fn patch_instruction(&mut self, at: usize, target: u32) {
        let instruction = self.code[at];
        if let Some(patched) = Instr::try_new(
            instruction.op(),
            instruction.a(),
            instruction.b(),
            instruction.c(),
            target,
        ) {
            self.code[at] = patched;
        } else {
            self.reject_packed_domain();
        }
    }

    fn reject_packed_domain(&mut self) {
        if !self.packed_domain_error {
            self.owner.reject(
                Span::default(),
                "function exceeds the packed instruction domain",
            );
            self.packed_domain_error = true;
        }
    }

    pub(super) fn literal(&mut self, value: Constant) -> Register {
        let dst = self.reg();
        let id = self.owner.constant(value);
        self.emit(Op::LoadConst, dst, 0, 0, id);
        dst
    }

    pub(super) fn emit_hoisted(&mut self, body: &[Statement<'_>]) {
        for statement in body {
            if let Statement::FunctionDeclaration(function) = statement {
                let Some(name) = &function.id else { continue };
                let params = Self::params(function, self.owner);
                let Some(body) = &function.body else { continue };
                let mut scopes = vec![Rc::clone(&self.local_slots)];
                scopes.extend(self.scopes.iter().cloned());
                let id = self.owner.compile_function(
                    Some(name.name.as_str()),
                    &params,
                    &body.statements,
                    &scopes,
                    Some(self.function_id),
                    Some(&function.params),
                );
                let dst = self.reg();
                self.emit(Op::MakeClosure, dst, 0, 0, id);
                let atom = self.owner.atom(name.name.as_str());
                self.store_atom(atom, dst);
                self.release_temporaries();
            }
        }
    }

    fn params<'c>(
        function: &'c oxc_ast::ast::Function<'c>,
        owner: &mut Compiler<'_>,
    ) -> Vec<&'c str> {
        Self::params_from_formals(&function.params, owner)
    }

    pub(super) fn params_from_formals<'c>(
        params: &'c oxc_ast::ast::FormalParameters<'c>,
        owner: &mut Compiler<'_>,
    ) -> Vec<&'c str> {
        if params.rest.is_some() {
            owner.reject(Span::default(), "rest parameters are unsupported");
        }
        params
            .items
            .iter()
            .filter_map(|item| match &item.pattern {
                BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                _ => {
                    owner.reject(item.span, "parameter pattern is unsupported");
                    None
                }
            })
            .collect()
    }

    fn static_key<'c>(key: &'c PropertyKey<'c>) -> Option<&'c str> {
        match key {
            PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
            PropertyKey::StringLiteral(value) => Some(value.value.as_str()),
            _ => None,
        }
    }

    pub(super) fn emit_parameter_defaults(&mut self, params: &oxc_ast::ast::FormalParameters<'_>) {
        for item in &params.items {
            let Some(initializer) = &item.initializer else {
                continue;
            };
            let BindingPattern::BindingIdentifier(id) = &item.pattern else {
                continue;
            };
            let atom = self.owner.atom(id.name.as_str());
            let current = self.load_atom(atom);
            let undefined = self.literal(Constant::Undefined);
            let missing =
                self.emit_binary(2, Operand::register(current), Operand::register(undefined));
            let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
            let value = self.expression(initializer);
            self.store_atom(atom, value);
            self.patch(skip);
        }
    }

    pub(super) fn release_temporaries(&mut self) {
        self.next_reg = 0;
    }
}
