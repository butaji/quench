use super::*;

mod array_literal;
mod call;
mod destructure;
mod disposal;
mod expression;
mod iteration;
mod object;
mod optional;
mod super_ops;
mod statement;
mod try_statement;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlKind {
    Loop,
    Switch,
    Label,
}

enum UpdateTarget {
    Name(Atom),
    Field(Atom, Register),
    Index(Register, Register),
}

struct ControlTarget {
    kind: ControlKind,
    label: Option<Atom>,
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

pub(super) struct FinallyContext {
    return_atom: Atom,
    return_edges: Vec<usize>,
    abrupt_edges: Vec<FinallyAbrupt>,
}

#[derive(Clone, Copy)]
pub(super) struct FinallyAbrupt {
    edge: usize,
    control: usize,
    continue_edge: bool,
}

pub(super) struct FunctionCompiler<'a, 'b> {
    pub(super) owner: &'a mut Compiler<'b>,
    pub(super) locals: Vec<Atom>,
    pub(super) code: Vec<Instr>,
    pub(super) wide: Vec<WideInstruction>,
    pub(super) next_reg: Register,
    pub(super) max_reg: Register,
    pub(super) local_slots: Rc<FxHashMap<Atom, u16>>,
    pub(super) scopes: Vec<Rc<FxHashMap<Atom, u16>>>,
    pub(super) function_id: u32,
    pub(super) handlers: Vec<crate::bytecode::Handler>,
    controls: Vec<ControlTarget>,
    iterator_closures: Vec<Atom>,
    pub(super) finally_contexts: Vec<FinallyContext>,
    packed_domain_error: bool,
    pub(super) super_static: bool,
    pub(super) super_home: bool,
    pub(super) super_home_atom: Option<Atom>,
    pub(super) async_function: bool,
    pub(super) generator: bool,
    pub(super) strict: bool,
    lexical_scopes: Vec<FxHashMap<Atom, Atom>>,
    disposable_stack: Option<Atom>,
}

impl<'a, 'b> FunctionCompiler<'a, 'b> {
    pub(super) fn new(
        owner: &'a mut Compiler<'b>,
        locals: Vec<Atom>,
        scopes: Vec<Rc<FxHashMap<Atom, u16>>>,
        function_id: u32,
        super_flags: (bool, bool),
        async_function: bool,
        generator: bool,
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
            wide: vec![],
            next_reg: 0,
            max_reg: 0,
            local_slots,
            scopes,
            function_id,
            handlers: vec![],
            controls: vec![],
            iterator_closures: vec![],
            finally_contexts: vec![],
            packed_domain_error: false,
            super_static: super_flags.0,
            super_home: super_flags.1,
            super_home_atom: None,
            async_function,
            generator,
            strict: false,
            lexical_scopes: Vec::new(),
            disposable_stack: None,
        }
    }

    pub(super) fn reg(&mut self) -> Register {
        let value = self.next_reg;
        if value >= SET_THIS_REGISTER {
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
            let index = self.wide.len();
            self.wide.push(WideInstruction::new(op, a, b, c, imm));
            Instr::wide(index).unwrap_or_else(|| {
                self.wide.pop();
                self.reject_packed_domain();
                Instr::new(Op::Nop, 0, 0, 0, 0)
            })
        });
        self.code.push(instruction);
        self.code.len() - 1
    }

    pub(super) fn patch(&mut self, at: usize) {
        let target = self.code.len() as u32;
        self.patch_instruction(at, target);
    }

    pub(super) fn patch_instruction(&mut self, at: usize, target: u32) {
        let instruction = self.code[at];
        if instruction.is_wide() {
            if let Some(wide) = self.wide.get_mut(instruction.wide_index()) {
                wide.set_imm(target);
            } else {
                self.reject_packed_domain();
            }
            return;
        }
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
                let mut scopes = self.capture_scopes();
                scopes.extend(self.scopes.iter().cloned());
                let id = self.owner.compile_function(
                    Some(name.name.as_str()),
                    &params,
                    &body.statements,
                    &scopes,
                    Some(self.function_id),
                    FunctionOptions {
                        defaults: Some(&function.params),
                        async_function: function.r#async,
                        generator: function.generator,
                        instance_fields: None,
                        super_static: false,
                        super_home: false,
                        super_home_atom: None,
                        rest_override: false,
                        implicit_super: false,
                        strict: body
                            .directives
                            .iter()
                            .any(|directive| directive.directive == "use strict"),
                    },
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
    ) -> Vec<String> {
        Self::params_from_formals(&function.params, owner)
    }

    pub(super) fn params_from_formals<'c>(
        params: &'c oxc_ast::ast::FormalParameters<'c>,
        _owner: &mut Compiler<'_>,
    ) -> Vec<String> {
        let mut result = params
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| Self::parameter_name(&item.pattern, index))
            .collect::<Vec<_>>();
        if let Some(rest) = &params.rest {
            result.push(Self::parameter_name(
                &rest.rest.argument,
                params.items.len(),
            ));
        }
        result
    }

    fn parameter_name(pattern: &BindingPattern<'_>, index: usize) -> String {
        Self::first_binding_name(pattern)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("\0rqj:param:{index}"))
    }

    fn first_binding_name<'c>(pattern: &'c BindingPattern<'c>) -> Option<&'c str> {
        match pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            BindingPattern::ObjectPattern(object) => object
                .properties
                .iter()
                .find_map(|property| Self::first_binding_name(&property.value))
                .or_else(|| {
                    object
                        .rest
                        .as_ref()
                        .and_then(|rest| Self::first_binding_name(&rest.argument))
                }),
            BindingPattern::ArrayPattern(array) => array
                .elements
                .iter()
                .flatten()
                .find_map(Self::first_binding_name)
                .or_else(|| {
                    array
                        .rest
                        .as_ref()
                        .and_then(|rest| Self::first_binding_name(&rest.argument))
                }),
            BindingPattern::AssignmentPattern(assignment) => {
                Self::first_binding_name(&assignment.left)
            }
        }
    }

    pub(super) fn hidden_local(&mut self, name: &str) -> Atom {
        let mut candidate = name.to_owned();
        while self.local_slots.contains_key(&self.owner.atom(&candidate)) {
            candidate.push('_');
        }
        let atom = self.owner.atom(&candidate);
        let slot = self.locals.len() as u16;
        Rc::get_mut(&mut self.local_slots)
            .expect("function local scope is uniquely owned")
            .insert(atom, slot);
        self.locals.push(atom);
        atom
    }

    pub(super) fn emit_implicit_super(&mut self) {
        let base = self.load_name("\0rqj:super");
        let apply = self.reg();
        let atom = self.owner.atom("apply");
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            apply,
            FieldBase::register(base).0,
            cache,
            atom,
        );
        let base_args = self.next_reg;
        let this = self.reg();
        self.emit(Op::LoadThis, this, 0, 0, 0);
        let args = self.load_name("\0rqj:derived-args");
        let args_arg = self.reg();
        self.emit(Op::Move, args_arg, args, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            apply,
            base,
            (u32::from(base_args) << 16) | 2,
        );
    }

    fn static_key<'c>(key: &'c PropertyKey<'c>) -> Option<&'c str> {
        match key {
            PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
            PropertyKey::StringLiteral(value)
                if !matches!(super::string::constant(value), Constant::StringUnits(_)) =>
            {
                Some(value.value.as_str())
            }
            _ => None,
        }
    }

    pub(super) fn emit_parameter_bindings(&mut self, params: &oxc_ast::ast::FormalParameters<'_>) {
        for (index, item) in params.items.iter().enumerate() {
            let atom = self.owner.atom(&Self::parameter_name(&item.pattern, index));
            let current = self.load_atom(atom);
            if matches!(&item.pattern, BindingPattern::BindingIdentifier(_)) {
                if let Some(initializer) = &item.initializer {
                    let undefined = self.literal(Constant::Undefined);
                    let missing = self.emit_binary(
                        2,
                        Operand::register(current),
                        Operand::register(undefined),
                    );
                    let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
                    let value = self.expression(initializer);
                    self.store_atom(atom, value);
                    self.patch(skip);
                }
            } else {
                self.bind_pattern(&item.pattern, current);
            }
        }
        if let Some(rest) = &params.rest {
            let atom = self.owner.atom(&Self::parameter_name(
                &rest.rest.argument,
                params.items.len(),
            ));
            let current = self.load_atom(atom);
            self.bind_pattern(&rest.rest.argument, current);
        }
    }

    pub(super) fn release_temporaries(&mut self) {
        self.next_reg = 0;
    }

    fn resolve_lexical(&self, atom: Atom) -> Atom {
        self.lexical_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&atom).copied())
            .unwrap_or(atom)
    }

    pub(super) fn push_lexical_scope(&mut self, body: &[Statement<'_>]) {
        let mut scope = FxHashMap::default();
        for statement in body {
            if let Statement::VariableDeclaration(declaration) = statement
                && matches!(
                    declaration.kind,
                    VariableDeclarationKind::Let | VariableDeclarationKind::Const
                )
            {
                for item in &declaration.declarations {
                    self.map_pattern_lexicals(&item.id, &mut scope);
                }
            }
        }
        self.lexical_scopes.push(scope);
    }

    pub(super) fn scoped_statements(&mut self, body: &[Statement<'_>]) {
        self.push_lexical_scope(body);
        self.statements(body);
        self.lexical_scopes.pop();
    }

    pub(super) fn capture_scopes(&self) -> Vec<Rc<FxHashMap<Atom, u16>>> {
        let mut scope = (*self.local_slots).clone();
        for lexical in &self.lexical_scopes {
            for (source, target) in lexical {
                if let Some(slot) = self.local_slots.get(target).copied() {
                    scope.insert(*source, slot);
                }
            }
        }
        let mut scopes = vec![Rc::new(scope)];
        scopes.extend(self.scopes.iter().cloned());
        scopes
    }

    pub(super) fn push_catch_binding(&mut self, handler: &CatchClause<'_>, binding: Option<Atom>) {
        let mut scope = FxHashMap::default();
        if let (Some(BindingPattern::BindingIdentifier(identifier)), Some(binding)) = (
            handler.param.as_ref().map(|parameter| &parameter.pattern),
            binding,
        ) {
            scope.insert(self.owner.atom(identifier.name.as_str()), binding);
        }
        self.lexical_scopes.push(scope);
    }

    fn map_pattern_lexicals(
        &mut self,
        pattern: &BindingPattern<'_>,
        scope: &mut FxHashMap<Atom, Atom>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(identifier) => {
                let atom = self.owner.atom(identifier.name.as_str());
                scope
                    .entry(atom)
                    .or_insert_with(|| self.hidden_local(identifier.name.as_str()));
            }
            BindingPattern::ObjectPattern(pattern) => {
                for property in &pattern.properties {
                    self.map_pattern_lexicals(&property.value, scope);
                }
                if let Some(rest) = &pattern.rest {
                    self.map_pattern_lexicals(&rest.argument, scope);
                }
            }
            BindingPattern::ArrayPattern(pattern) => {
                for element in pattern.elements.iter().flatten() {
                    self.map_pattern_lexicals(element, scope);
                }
                if let Some(rest) = &pattern.rest {
                    self.map_pattern_lexicals(&rest.argument, scope);
                }
            }
            BindingPattern::AssignmentPattern(pattern) => {
                self.map_pattern_lexicals(&pattern.left, scope)
            }
        }
    }
}
