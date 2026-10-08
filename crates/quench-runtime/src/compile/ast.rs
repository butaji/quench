use super::*;

mod array_literal;
mod call;
mod destructure;
mod disposal;
mod expression;
mod iteration;
mod object;
mod optional;
mod statement;
mod super_ops;
mod try_statement;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlKind {
    Loop,
    Switch,
    Label,
}

#[derive(Clone, Copy)]
pub(super) enum StatementCompletion {
    Ignored,
    Track(Register),
    Suppress(Register),
    Finally { current: Register, target: Register },
}

impl StatementCompletion {
    pub(super) fn register(self) -> Option<Register> {
        match self {
            Self::Ignored => None,
            Self::Track(register) | Self::Suppress(register) => Some(register),
            Self::Finally { current, .. } => Some(current),
        }
    }
}

struct ControlTarget {
    kind: ControlKind,
    label: Option<Atom>,
    with_depth: u16,
    breaks: Vec<usize>,
    continues: Vec<usize>,
    break_destinations: Vec<Rc<std::cell::Cell<Option<u32>>>>,
    continue_destinations: Vec<Rc<std::cell::Cell<Option<u32>>>>,
}

#[derive(Clone, Copy)]
struct IteratorClosure {
    iterator: Atom,
    control_depth: usize,
}

struct LexicalScope {
    bindings: FxHashMap<Atom, Atom>,
    kinds: FxHashMap<Atom, LexicalBindingKind>,
    catch_parameter: bool,
    pub(super) with_depth: u16,
}

pub(super) struct FinallyContext {
    control_depth: usize,
    iterator_depth: usize,
    return_atom: Atom,
    return_edges: Vec<usize>,
    abrupt_edges: Vec<FinallyAbrupt>,
}

#[derive(Clone)]
pub(super) struct FinallyAbrupt {
    edge: usize,
    control: usize,
    destination: Rc<std::cell::Cell<Option<u32>>>,
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
    function_scope: FxHashSet<Atom>,
    annex_b_collisions: FxHashSet<u32>,
    pub(super) scopes: Vec<Rc<FxHashMap<Atom, u16>>>,
    pub(super) function_id: u32,
    pub(super) handlers: Vec<crate::bytecode::Handler>,
    pub(super) binding_sites: Vec<crate::bytecode::BindingSite>,
    pub(super) environment_clones: Vec<Vec<u16>>,
    controls: Vec<ControlTarget>,
    iterator_closures: Vec<IteratorClosure>,
    pub(super) iterator_close_ranges: Vec<(u32, u32)>,
    iterator_close_exclusions: Vec<Vec<(u32, u32)>>,
    pub(super) finally_contexts: Vec<FinallyContext>,
    packed_domain_error: bool,
    pub(super) super_static: bool,
    pub(super) super_home: bool,
    pub(super) super_home_atom: Option<Atom>,
    pub(super) async_function: bool,
    pub(super) generator: bool,
    pub(super) this_override: Option<Register>,
    pub(super) class_field_initializer: bool,
    pub(super) optional_chain_end_edges: Option<Vec<usize>>,
    pub(super) with_depth: u16,
    inherited_with_depth: u16,
    pub(super) strict: bool,
    pub(super) dynamic_eval: bool,
    parameter_context: bool,
    pub(super) parameter_eval_arguments_error: bool,
    pub(super) parameter_arguments_slot: Option<u16>,
    pub(super) arguments_slot: Option<u16>,
    pub(super) statement_completion: StatementCompletion,
    pub(super) parameter_local_count: usize,
    pub(super) super_call_binds_this: bool,
    lexical_scopes: Vec<LexicalScope>,
    disposal_scopes: Vec<DisposalScope>,
}

#[derive(Default)]
pub(super) struct DisposalScope {
    stack: Option<Atom>,
    asynchronous: bool,
}

impl<'a, 'b> FunctionCompiler<'a, 'b> {
    pub(super) fn new(
        owner: &'a mut Compiler<'b>,
        locals: Vec<Atom>,
        function_scope: FxHashSet<Atom>,
        annex_b_collisions: FxHashSet<u32>,
        scopes: Vec<Rc<FxHashMap<Atom, u16>>>,
        function_id: u32,
        super_flags: (bool, bool),
        async_function: bool,
        generator: bool,
        parameter_arguments_slot: Option<u16>,
        arguments_slot: Option<u16>,
        parameter_local_count: usize,
        with_depth: u16,
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
            function_scope,
            annex_b_collisions,
            scopes,
            function_id,
            handlers: vec![],
            binding_sites: vec![],
            environment_clones: vec![],
            controls: vec![],
            iterator_closures: vec![],
            iterator_close_ranges: vec![],
            iterator_close_exclusions: vec![],
            finally_contexts: vec![],
            packed_domain_error: false,
            super_static: super_flags.0,
            super_home: super_flags.1,
            super_home_atom: None,
            async_function,
            generator,
            this_override: None,
            class_field_initializer: false,
            optional_chain_end_edges: None,
            with_depth,
            inherited_with_depth: with_depth,
            strict: false,
            dynamic_eval: false,
            parameter_context: false,
            parameter_eval_arguments_error: false,
            parameter_arguments_slot,
            arguments_slot,
            statement_completion: StatementCompletion::Ignored,
            parameter_local_count,
            super_call_binds_this: false,
            lexical_scopes: Vec::new(),
            disposal_scopes: vec![DisposalScope::default()],
        }
    }

    pub(super) fn reg(&mut self) -> Register {
        let value = self.next_reg;
        if value >= SET_THIS_REGISTER {
            self.reject_packed_domain("register allocation exceeds its encoded domain");
            return 0;
        }
        self.next_reg += 1;
        self.max_reg = self.max_reg.max(self.next_reg);
        value
    }

    pub(super) fn clear_statement_completion(&mut self) {
        let target = match self.statement_completion {
            StatementCompletion::Track(target) => target,
            StatementCompletion::Finally { current, .. } => current,
            StatementCompletion::Ignored | StatementCompletion::Suppress(_) => return,
        };
        let undefined = self.literal(Constant::Undefined);
        self.emit(Op::Move, target, undefined, 0, 0);
    }

    fn record_statement_completion(&mut self, value: Register) {
        match self.statement_completion {
            StatementCompletion::Track(target) => {
                self.emit(Op::Move, target, value, 0, 0);
            }
            StatementCompletion::Finally { current, .. } => {
                self.emit(Op::Move, current, value, 0, 0);
            }
            StatementCompletion::Ignored | StatementCompletion::Suppress(_) => {}
        }
    }

    pub(super) fn scoped_statements_without_completion(&mut self, body: &[Statement<'_>]) {
        let previous = self.statement_completion;
        self.statement_completion = match previous {
            StatementCompletion::Track(register) | StatementCompletion::Suppress(register) => {
                StatementCompletion::Suppress(register)
            }
            StatementCompletion::Finally { current, .. } => StatementCompletion::Suppress(current),
            StatementCompletion::Ignored => StatementCompletion::Ignored,
        };
        self.scoped_statements(body);
        self.statement_completion = previous;
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
            self.store_wide_instruction(WideInstruction::new(op, a, b, c, imm))
                .unwrap_or_else(|| {
                    self.reject_packed_domain("wide instruction index exceeds its encoded domain");
                    Instr::new(Op::Nop, 0, 0, 0, 0)
                })
        });
        self.code.push(instruction);
        if op == Op::MakeClosure && self.with_depth != self.inherited_with_depth {
            self.record_lexical_binding_site(self.code.len() - 1);
        }
        // Deletion always resolves a binding by name, including block slots
        // whose storage atom differs from their source name.
        if op == Op::DeleteName
            || (self.with_depth != self.inherited_with_depth
                && matches!(
                    op,
                    Op::LoadName
                        | Op::LoadNameCall
                        | Op::LoadNameTypeof
                        | Op::ResolveName
                        | Op::StoreName
                ))
        {
            self.record_name_binding_site(self.code.len() - 1, imm);
        }
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
                wide.set_jump_target(target);
            } else {
                self.reject_packed_domain("wide instruction target is missing");
            }
            return;
        }
        let mut patched = instruction;
        if patched.try_set_jump_target(target) {
            self.code[at] = patched;
        } else if let Some(wide) = self.store_wide_instruction({
            let mut wide = patched.as_wide();
            wide.set_jump_target(target);
            wide
        }) {
            self.code[at] = wide;
        } else {
            self.reject_packed_domain("jump target exceeds its encoded domain");
        }
    }

    fn store_wide_instruction(&mut self, instruction: WideInstruction) -> Option<Instr> {
        let index = self.wide.len();
        self.wide.push(instruction);
        Instr::wide(index).or_else(|| {
            self.wide.pop();
            None
        })
    }

    fn reject_packed_domain(&mut self, reason: &str) {
        if !self.packed_domain_error {
            self.owner.reject(Span::default(), reason);
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
            let statement = super::early::statement_without_labels(statement);
            let (function, default_export) = match statement {
                Statement::FunctionDeclaration(function) => (Some(function), false),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    oxc_ast::ast::Declaration::FunctionDeclaration(function) => {
                        (Some(function), false)
                    }
                    _ => (None, false),
                },
                Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                    oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                        (Some(function), true)
                    }
                    _ => (None, false),
                },
                _ => (None, false),
            };
            if let Some(function) = function {
                let name = function
                    .id
                    .as_ref()
                    .map(|name| name.name.as_str())
                    .or(default_export.then_some("default"));
                let Some(name) = name else { continue };
                let params = Self::params(function, self.owner);
                let Some(body) = &function.body else { continue };
                let scopes = self.capture_scopes();
                let id = self.owner.compile_function(
                    Some(name),
                    &params,
                    FunctionBody::Statements(&body.statements),
                    &scopes,
                    Some(self.function_id),
                    FunctionOptions {
                        defaults: Some(&function.params),
                        source_text: self.owner.source_text(function.span),
                        name_binding: None,
                        async_function: function.r#async,
                        generator: function.generator,
                        class_constructor: false,
                        derived_constructor: false,
                        non_constructible: false,
                        class_field_initializer: false,
                        instance_fields: None,
                        instance_private_methods: None,
                        super_static: false,
                        super_home: false,
                        super_home_atom: None,
                        rest_override: false,
                        implicit_super: false,
                        with_depth: self.with_depth,
                        strict: self.strict
                            || body
                                .directives
                                .iter()
                                .any(|directive| directive.directive == "use strict"),
                    },
                );
                let dst = self.reg();
                self.emit(Op::MakeClosure, dst, 0, 0, id);
                if default_export {
                    let binding = function
                        .id
                        .as_ref()
                        .map(|identifier| identifier.name.to_string())
                        .unwrap_or_else(|| super::module_default_binding(self.owner.source));
                    let atom = self.owner.atom(&binding);
                    self.store_atom_with_initialization(atom, dst, true);
                } else if let Some(identifier) = &function.id {
                    let atom = self.owner.atom(identifier.name.as_str());
                    if self.function_id == 0
                        && self.owner.eval_context.is_some()
                        && self.active_lexical_binding(atom).is_none()
                        && let Some(slot) = self.local_slots.get(&atom).copied()
                    {
                        self.emit(Op::StoreVarBinding, dst, 0, 0, u32::from(slot));
                    } else {
                        self.initialize_atom(atom, dst);
                    }
                } else {
                    continue;
                }
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
        Self::parameter_local_names(params)
    }

    fn parameter_local_names(params: &oxc_ast::ast::FormalParameters<'_>) -> Vec<String> {
        let non_simple = Self::has_non_simple_parameters(params);
        let mut result = params
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| Self::parameter_name(&item.pattern, index, non_simple))
            .collect::<Vec<_>>();
        if let Some(rest) = &params.rest {
            result.push(Self::parameter_name(
                &rest.rest.argument,
                params.items.len(),
                non_simple,
            ));
        }
        result
    }

    pub(super) fn has_non_simple_parameters(params: &oxc_ast::ast::FormalParameters<'_>) -> bool {
        params.rest.is_some()
            || params.items.iter().any(|item| {
                item.initializer.is_some()
                    || !matches!(item.pattern, BindingPattern::BindingIdentifier(_))
            })
    }

    fn parameter_name(pattern: &BindingPattern<'_>, index: usize, hidden: bool) -> String {
        if hidden {
            return format!("\0rqj:param:{index}");
        }
        match pattern {
            BindingPattern::BindingIdentifier(id) => id.name.to_string(),
            _ => format!("\0rqj:param:{index}"),
        }
    }

    pub(super) fn parameter_bound_names(
        params: &oxc_ast::ast::FormalParameters<'_>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for item in &params.items {
            Self::collect_binding_names(&item.pattern, &mut names);
        }
        if let Some(rest) = &params.rest {
            Self::collect_binding_names(&rest.rest.argument, &mut names);
        }
        names
    }

    fn collect_binding_names(pattern: &BindingPattern<'_>, names: &mut Vec<String>) {
        match pattern {
            BindingPattern::BindingIdentifier(identifier) => {
                names.push(identifier.name.to_string());
            }
            BindingPattern::AssignmentPattern(pattern) => {
                Self::collect_binding_names(&pattern.left, names);
            }
            BindingPattern::ArrayPattern(pattern) => {
                for element in pattern.elements.iter().flatten() {
                    Self::collect_binding_names(element, names);
                }
                if let Some(rest) = &pattern.rest {
                    Self::collect_binding_names(&rest.argument, names);
                }
            }
            BindingPattern::ObjectPattern(pattern) => {
                for property in &pattern.properties {
                    Self::collect_binding_names(&property.value, names);
                }
                if let Some(rest) = &pattern.rest {
                    Self::collect_binding_names(&rest.argument, names);
                }
            }
        }
    }

    pub(super) fn anonymous_function_definition(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::FunctionExpression(function) => function.id.is_none(),
            Expression::ArrowFunctionExpression(_) => true,
            Expression::ClassExpression(class) => class.id.is_none(),
            Expression::ParenthesizedExpression(expression) => {
                Self::anonymous_function_definition(&expression.expression)
            }
            _ => false,
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
        let args = self.load_name("\0rqj:derived-args");
        let callee = self.super_constructor();
        let result = self.reg();
        self.emit(
            Op::Construct,
            result,
            callee,
            args,
            crate::bytecode::ImmediateLayout::construct_immediate(1, true, true),
        );
        self.emit(Op::InitializeThis, result, 0, 0, 0);
        self.emit(Op::Return, result, 0, 0, 0);
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
        let non_simple = Self::has_non_simple_parameters(params);
        let parameter_names = Self::parameter_local_names(params);
        if non_simple {
            for name in Self::parameter_bound_names(params) {
                let atom = self.owner.atom(&name);
                if let Some(slot) = self.local_slots.get(&atom).copied() {
                    self.emit(Op::InitializeTdz, 0, 0, 0, u32::from(slot));
                }
            }
        }
        for (index, item) in params.items.iter().enumerate() {
            let atom = self.owner.atom(&parameter_names[index]);
            let current = self.load_atom(atom);
            if let Some(initializer) = &item.initializer {
                let undefined = self.literal(Constant::Undefined);
                let missing =
                    self.emit_binary(2, Operand::register(current), Operand::register(undefined));
                let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
                let previous = self.parameter_context;
                self.parameter_context = true;
                let value = self.expression(initializer);
                self.parameter_context = previous;
                if let BindingPattern::BindingIdentifier(identifier) = &item.pattern
                    && Self::anonymous_function_definition(initializer)
                {
                    let name = self.owner.atom(identifier.name.as_str());
                    self.emit(Op::SetFunctionName, value, 0, 0, name);
                }
                self.emit(Op::Move, current, value, 0, 0);
                self.patch(skip);
            }
            if matches!(&item.pattern, BindingPattern::BindingIdentifier(_)) {
                let BindingPattern::BindingIdentifier(identifier) = &item.pattern else {
                    unreachable!();
                };
                let binding = self.owner.atom(identifier.name.as_str());
                self.initialize_atom(binding, current);
            } else {
                self.bind_pattern(&item.pattern, current);
            }
        }
        if let Some(rest) = &params.rest {
            let atom = self.owner.atom(&Self::parameter_name(
                &rest.rest.argument,
                params.items.len(),
                non_simple,
            ));
            let current = self.load_atom(atom);
            self.bind_pattern(&rest.rest.argument, current);
        }
    }

    pub(super) fn release_temporaries(&mut self) {
        self.next_reg = self
            .statement_completion
            .register()
            .map_or(0, |register| register + 1);
    }

    fn active_lexical_binding(&self, atom: Atom) -> Option<Atom> {
        self.lexical_scopes
            .iter()
            .rev()
            .filter(|scope| self.with_depth == 0 || scope.with_depth >= self.with_depth)
            .find_map(|scope| scope.bindings.get(&atom).copied())
    }

    fn resolve_lexical(&self, atom: Atom) -> Atom {
        self.active_lexical_binding(atom).unwrap_or(atom)
    }

    fn is_compiler_binding(&self, atom: Atom) -> bool {
        self.owner.atoms[atom as usize].starts_with('\0')
    }

    pub(super) fn needs_strict_global_reference_capture(&mut self, atom: Atom) -> bool {
        if !self.strict || self.owner.atoms[atom as usize].as_ref().starts_with('\0') {
            return false;
        }
        let binding = self.resolve_lexical(atom);
        !self.local_slots.contains_key(&binding)
            && !self.scopes.iter().any(|scope| scope.contains_key(&binding))
            && !self.has_immutable_capture(binding)
    }

    pub(super) fn push_lexical_bindings(&mut self, bindings: FxHashMap<Atom, Atom>) {
        self.push_lexical_bindings_with_immutability(bindings, FxHashSet::default());
    }

    pub(super) fn push_body_lexical_bindings(&mut self, bindings: &[(Atom, LexicalBindingKind)]) {
        self.push_immutable_lexical_bindings(
            bindings.iter().map(|(atom, _)| (*atom, *atom)).collect(),
            bindings
                .iter()
                .filter_map(|(atom, kind)| {
                    (*kind == LexicalBindingKind::Immutable).then_some(*atom)
                })
                .collect(),
        );
    }

    pub(super) fn push_immutable_lexical_bindings(
        &mut self,
        bindings: FxHashMap<Atom, Atom>,
        immutable: FxHashSet<Atom>,
    ) {
        self.push_lexical_bindings_with_immutability(bindings, immutable);
    }

    pub(super) fn map_declaration_lexicals(
        &mut self,
        declaration: &VariableDeclaration<'_>,
        bindings: &mut FxHashMap<Atom, Atom>,
        immutable: &mut FxHashSet<Atom>,
    ) {
        for item in &declaration.declarations {
            self.map_pattern_lexicals(&item.id, bindings);
            if super::is_immutable_binding_declaration(declaration.kind) {
                let mut names = Vec::new();
                super::early::collect_pattern_names(&item.id, &mut names);
                immutable.extend(names.iter().map(|name| self.owner.atom(name)));
            }
        }
    }

    fn push_lexical_bindings_with_immutability(
        &mut self,
        bindings: FxHashMap<Atom, Atom>,
        immutable: FxHashSet<Atom>,
    ) {
        let kinds = bindings
            .keys()
            .map(|atom| {
                (
                    *atom,
                    if immutable.contains(atom) {
                        LexicalBindingKind::Immutable
                    } else {
                        LexicalBindingKind::Mutable
                    },
                )
            })
            .collect();
        self.lexical_scopes.push(LexicalScope {
            bindings,
            kinds,
            catch_parameter: false,
            with_depth: self.with_depth,
        });
    }

    pub(super) fn push_function_name_binding(&mut self, name: Atom, binding: Atom) {
        self.lexical_scopes.push(LexicalScope {
            bindings: FxHashMap::from_iter([(name, binding)]),
            kinds: FxHashMap::from_iter([(name, LexicalBindingKind::FunctionName)]),
            catch_parameter: false,
            with_depth: self.with_depth,
        });
    }

    pub(super) fn lexical_binding_kind(&self, atom: Atom) -> LexicalBindingKind {
        self.lexical_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.kinds.get(&atom).copied())
            .unwrap_or(LexicalBindingKind::Mutable)
    }

    fn lexical_scope_slots(&self) -> Vec<u16> {
        self.lexical_scopes
            .last()
            .into_iter()
            .flat_map(|scope| scope.bindings.values())
            .filter_map(|binding| self.local_slots.get(binding).copied())
            .collect()
    }

    pub(super) fn clone_lexical_environment(&mut self) {
        self.clone_environment_slots(self.lexical_scope_slots());
    }

    pub(super) fn clone_environment_slots(&mut self, mut slots: Vec<u16>) {
        slots.sort_unstable();
        slots.dedup();
        if slots.is_empty() {
            return;
        }
        let index = if let Some(index) = self
            .environment_clones
            .iter()
            .position(|existing| *existing == slots)
        {
            index
        } else {
            self.environment_clones.push(slots);
            self.environment_clones.len() - 1
        };
        self.emit(Op::CloneEnv, 0, 0, 0, index as u32);
    }

    pub(super) fn initialize_lexical_scope(&mut self) {
        let slots = self.lexical_scope_slots();
        self.clone_environment_slots(slots.clone());
        self.initialize_tdz_slots(slots);
    }

    pub(super) fn initialize_tdz_slots(&mut self, mut slots: Vec<u16>) {
        slots.sort_unstable();
        slots.dedup();
        for slot in slots {
            self.emit(Op::InitializeTdz, 0, 0, 0, u32::from(slot));
        }
    }

    pub(super) fn pop_lexical_scope(&mut self) {
        self.lexical_scopes.pop();
    }

    pub(super) fn push_lexical_scope(&mut self, body: &[Statement<'_>]) {
        let mut scope = FxHashMap::default();
        let mut immutable = FxHashSet::default();
        for statement in body {
            self.collect_lexical_binding(statement, &mut scope, &mut immutable);
        }
        self.push_immutable_lexical_bindings(scope, immutable);
        self.initialize_lexical_scope();
    }

    pub(super) fn push_switch_lexical_scope(&mut self, cases: &[SwitchCase<'_>]) {
        let mut scope = FxHashMap::default();
        let mut immutable = FxHashSet::default();
        for statement in cases.iter().flat_map(|case| &case.consequent) {
            self.collect_lexical_binding(statement, &mut scope, &mut immutable);
        }
        self.push_immutable_lexical_bindings(scope, immutable);
        self.initialize_lexical_scope();
    }

    fn collect_lexical_binding(
        &mut self,
        statement: &Statement<'_>,
        scope: &mut FxHashMap<Atom, Atom>,
        immutable: &mut FxHashSet<Atom>,
    ) {
        match super::early::statement_without_labels(statement) {
            Statement::VariableDeclaration(declaration)
                if super::is_lexical_binding_declaration(declaration.kind) =>
            {
                self.map_declaration_lexicals(declaration, scope, immutable);
            }
            Statement::ClassDeclaration(class) => {
                if let Some(identifier) = &class.id {
                    let source = self.owner.atom(identifier.name.as_str());
                    let target =
                        self.hidden_local(&format!("\0rqj:block-class:{}", identifier.name));
                    scope.insert(source, target);
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id {
                    let source = self.owner.atom(identifier.name.as_str());
                    let target =
                        self.hidden_local(&format!("\0rqj:block-function:{}", identifier.name));
                    scope.insert(source, target);
                }
            }
            _ => {}
        }
    }

    pub(super) fn scoped_statements(&mut self, body: &[Statement<'_>]) {
        let has_using = Self::has_using_declarations(body);
        let disposal_error = has_using.then(|| self.hidden_local("\0rqj:using-error"));
        if has_using {
            self.push_disposal_scope();
            self.push_disposal_context();
        }
        self.push_lexical_scope(body);
        self.emit_hoisted(body);
        let disposal_body_start = self.code.len() as u32;
        self.statements(body);
        let disposal_body_end = self.code.len() as u32;
        self.lexical_scopes.pop();
        if let Some(error) = disposal_error {
            let context = self.finally_contexts.pop().expect("block disposal context");
            self.emit_disposal_scope_exit(
                disposal_body_start,
                disposal_body_end,
                error,
                Some(context),
            );
            self.pop_disposal_scope();
        }
    }

    pub(super) fn capture_scopes(&mut self) -> Vec<Rc<FxHashMap<Atom, u16>>> {
        let mut scope: FxHashMap<Atom, u16> = self
            .function_scope
            .iter()
            .filter(|_| !self.dynamic_eval)
            .filter_map(|atom| self.local_slots.get(atom).map(|slot| (*atom, *slot)))
            .collect();
        scope.extend(
            self.local_slots
                .iter()
                .filter(|(atom, _)| self.is_compiler_binding(**atom))
                .map(|(atom, slot)| (*atom, *slot)),
        );
        if self.parameter_context {
            scope.retain(|_, slot| {
                usize::from(*slot) < self.parameter_local_count
                    || Some(*slot) == self.parameter_arguments_slot
            });
            if let Some(slot) = self.parameter_arguments_slot {
                scope.insert(self.owner.atom("arguments"), slot);
            }
        }
        for lexical in &self.lexical_scopes {
            for (source, target) in &lexical.bindings {
                if let Some(slot) = self.local_slots.get(target).copied() {
                    scope.insert(*source, slot);
                    let kind = lexical
                        .kinds
                        .get(source)
                        .copied()
                        .unwrap_or(LexicalBindingKind::Mutable);
                    let name = self.owner.atoms[*source as usize].clone();
                    for kind in LexicalBindingKind::ALL {
                        let marker = format!("{}{name}", kind.capture_prefix());
                        if let Some(marker) = self.owner.atom_index.get(marker.as_str()) {
                            scope.remove(marker);
                        }
                    }
                    let catch_name = format!("\0rqj:catch-capture:{name}");
                    if let Some(marker) = self.owner.atom_index.get(catch_name.as_str()) {
                        scope.remove(marker);
                    }
                    let marker = self.owner.atom(&format!("{}{name}", kind.capture_prefix()));
                    scope.insert(marker, slot);
                    if lexical.catch_parameter {
                        let marker = self.owner.atom(&catch_name);
                        scope.insert(marker, slot);
                    }
                }
            }
        }
        let mut scopes = vec![Rc::new(scope)];
        // Eval can introduce vars that shadow inherited bindings. Current lexical
        // bindings cannot be replaced by those declarations and remain capturable.
        if !self.dynamic_eval {
            scopes.extend(self.scopes.iter().cloned());
        } else {
            // Source eval cannot introduce compiler bindings. Keep their scope
            // depths while dropping source-name projections and capture markers.
            scopes.extend(self.scopes.iter().map(|scope| {
                Rc::new(
                    scope
                        .iter()
                        .filter_map(|(atom, slot)| {
                            let name = self.owner.atoms[*atom as usize].as_ref();
                            (self.is_compiler_binding(*atom)
                                && !LexicalBindingKind::ALL
                                    .into_iter()
                                    .any(|kind| name.starts_with(kind.capture_prefix()))
                                && !name.starts_with("\0rqj:catch-capture:"))
                            .then_some((*atom, *slot))
                        })
                        .collect(),
                )
            }));
        }
        scopes
    }

    pub(super) fn push_catch_binding(&mut self, handler: &CatchClause<'_>, binding: Option<Atom>) {
        let mut scope = FxHashMap::default();
        let mut requires_initialization = false;
        if let Some(parameter) = &handler.param {
            match (&parameter.pattern, binding) {
                (BindingPattern::BindingIdentifier(identifier), Some(binding)) => {
                    scope.insert(self.owner.atom(identifier.name.as_str()), binding);
                }
                (pattern, _) => {
                    self.map_pattern_lexicals(pattern, &mut scope);
                    requires_initialization = true;
                }
            }
        }
        self.push_lexical_bindings(scope);
        // Annex B permits redeclaration only for a simple catch identifier.
        self.lexical_scopes
            .last_mut()
            .expect("catch binding scope was just pushed")
            .catch_parameter = !requires_initialization;
        if requires_initialization {
            self.initialize_lexical_scope();
        }
    }

    fn map_pattern_lexicals(
        &mut self,
        pattern: &BindingPattern<'_>,
        scope: &mut FxHashMap<Atom, Atom>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(identifier) => {
                let atom = self.owner.atom(identifier.name.as_str());
                scope.entry(atom).or_insert_with(|| {
                    self.hidden_local(&format!("\0rqj:block-binding:{}", identifier.name))
                });
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
