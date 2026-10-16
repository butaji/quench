use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

use crate::compile::eval_context_source;

impl<H: Host> Vm<H> {
    pub(super) fn eval_script_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let previous_global = self
            .active_native_env()
            .filter(|global| self.object_data(*global).is_some())
            .map(|global| self.switch_realm_global(global));
        let result = self.eval_script_native_in_realm(p, args);
        if let Some(previous_global) = previous_global {
            self.switch_realm_global(previous_global);
        }
        result
    }

    pub(super) fn eval_script_native_in_realm(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let source = self.to_string(p, source)?;
        let source_name = format!("<evalScript:{}>", self.programs.len());
        self.eval_script_source(p, &source, &source_name)
    }

    /// Evaluates `source` as a global Script of the active realm: its
    /// top-level lexical declarations join the realm's global lexical scope.
    pub(super) fn eval_script_source(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        source_name: &str,
    ) -> Result<Value, JsError> {
        let residual =
            match crate::Engine::specialize_script_with_atom_prefix(source, source_name, &*self) {
                Ok(residual) => residual,
                Err(diagnostics) => {
                    if diagnostics
                        .iter()
                        .any(crate::compile::Diagnostic::is_stack_exhausted)
                    {
                        return Err(self.stack_exhaustion_error());
                    }
                    let message = diagnostics
                        .first()
                        .map_or("invalid script source".to_owned(), ToString::to_string);
                    return self.syntax_error_result(p, &message);
                }
            };
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(p, "dynamic program store is full".into()));
        };
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "dynamic program is unavailable".into()))?;
        let eval_script_context = std::mem::replace(&mut self.eval_script_context, true);
        let active_program = self.replace_active_program(program_id);
        let result = (|| {
            self.instantiate_global_declarations(&residual)?;
            let root = self.closure(&residual, 0, Value::NULL)?;
            self.call_value(&residual, root, self.realm.globals, &[])
        })();
        self.set_active_program(active_program);
        self.eval_script_context = eval_script_context;
        result
    }

    pub(super) fn eval_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let previous_global = self
            .active_native_env()
            .filter(|global| self.object_data(*global).is_some())
            .map(|global| self.switch_realm_global(global));
        let result = self.eval_native_in_realm(p, args);
        if let Some(previous_global) = previous_global {
            self.switch_realm_global(previous_global);
        }
        result
    }

    fn eval_native_in_realm(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::String(source_text)) = self.heap.get(source).cloned()
            && let Some(literal) = crate::Engine::eval_single_regexp_literal(source_text.host_string())
        {
            let text = source_text.host_string();
            let start = text[..literal.span.start].encode_utf16().count() + 1;
            let end = text[..literal.span.end].encode_utf16().count() - literal.flags.len() - 1;
            let units = source_text.units().wide();
            let pattern_units = &units[start..end];
            let flag_units = literal.flags.encode_utf16().collect::<Vec<_>>();
            if let (Ok(pattern), Ok(flags)) = (
                String::from_utf16(&pattern_units),
                String::from_utf16(&flag_units),
            ) && quench_regexp::validate_property_escapes(&pattern, &flags)
                .is_err_and(|error| quench_regexp::is_stack_exhaustion_message(&error))
            {
                return Err(self.stack_exhaustion_error());
            }
            let pattern = self
                .heap
                .alloc(Cell::String(JsString::from_units(pattern_units)));
            let flags = self
                .heap
                .alloc(Cell::String(JsString::from_units(&flag_units)));
            return self.construct_regexp_native(p, &[pattern, flags], None);
        }
        let source_text = match self.heap.get(source) {
            Some(Cell::String(value)) => value.clone(),
            _ => return Ok(source),
        };
        let text = source_text.host_string().to_owned();
        let inherited_strict = self.direct_eval
            && (self.in_class_field_initializer(p)
                || self.frames.last()
                    .and_then(|frame| p.functions.get(frame.function as usize))
                    .is_some_and(|function| function.strict));
        self.validate_eval_arguments_context(p, &text)?;
        let source_name = format!("<Eval:{}>", self.programs.len());
        let cache_source = (!self.direct_eval).then_some(source_text);
        self.eval_global_script_with_cache(p, &text, inherited_strict, &source_name, cache_source)
    }

    pub(super) fn eval_global_script(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        strict: bool,
    ) -> Result<Value, JsError> {
        let source_name = format!("<Eval:{}>", self.programs.len());
        self.eval_global_script_named(p, source, strict, &source_name)
    }

    pub(super) fn eval_global_script_named(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        strict: bool,
        source_name: &str,
    ) -> Result<Value, JsError> {
        self.eval_global_script_with_cache(p, source, strict, source_name, None)
    }

    fn eval_global_script_with_cache(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        strict: bool,
        source_name: &str,
        cache_source: Option<JsString>,
    ) -> Result<Value, JsError> {
        if let Some(program) = cache_source.as_ref().and_then(|source| self.programs.cached_global_eval(source)) {
            return self.run_global_eval_program(p, program);
        }
        if strict {
            self.validate_strict_eval(p, source)?;
        }
        let context = self.direct_eval_context(p).unwrap_or_default();
        let private_names = if context.home_atom.is_some() {
            self.direct_eval_private_names(p)
                .map_or_else(Vec::new, |(_, names)| names)
        } else {
            Vec::new()
        };
        let caller_has_global_variables = self
            .frames
            .last()
            .is_none_or(|frame| frame.function == super::ROOT_FUNCTION_ID);
        let annex_b_forbidden_names = if self.direct_eval {
            (0..crate::compile::AtomPrefix::len(self) as Atom)
                .filter(|atom| {
                    (caller_has_global_variables
                        && self.realm.global_lexical_declarations.contains(atom))
                        || self
                            .direct_eval_binding(*atom)
                            .is_some_and(|binding| binding.conflicts_with_var())
                })
                .map(|atom| self.atom_name(atom).to_owned())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let residual = crate::Engine::specialize_eval_with_context(
            source,
            source_name,
            &*self,
            strict,
            context,
            &private_names,
            &annex_b_forbidden_names,
        )
        .map_err(|diagnostics| {
            if diagnostics
                .iter()
                .any(crate::compile::Diagnostic::is_stack_exhausted)
            {
                return self.stack_exhaustion_error();
            }
            let message = diagnostics
                .first()
                .map_or("invalid eval source".to_owned(), ToString::to_string);
            self.syntax_error_result(p, &message)
                .expect_err("dynamic eval syntax errors must throw")
        })?;
        let cache_source = cache_source.filter(|_| {
            residual.functions.first().is_some_and(|root| !root.strict)
                && !crate::Engine::eval_has_tagged_template(source)
        });
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(p, "dynamic program store is full".into()));
        };
        let origin = self.capture_eval_origin(p)?;
        self.programs.set_origin(program_id, super::program_store::CompilationOrigin::Eval { caller: Some(origin) });
        if let Some(source) = cache_source { self.programs.cache_global_eval(source, program_id); }
        self.run_global_eval_program(p, program_id)
    }

    pub(crate) fn evaluate_embedding_specialized_script(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        self.evaluate_embedding_specialized_script_with_origin(source, name, None)
    }

    pub(crate) fn evaluate_embedding_specialized_script_with_origin(
        &mut self,
        source: &str,
        name: &str,
        origin: Option<(u32, u32)>,
    ) -> Result<Value, JsError> {
        let prepared = self.prepare_embedding_specialized_script(source, name, origin)?;
        self.evaluate_embedding_prepared_script(&prepared)
    }

    pub(crate) fn prepare_embedding_specialized_script(
        &mut self, source: &str, name: &str, origin: Option<(u32, u32)>,
    ) -> Result<crate::PreparedScript, JsError> {
        let program = self.embedding_program()?;
        let mut residual = crate::Engine::specialize_script_with_atom_prefix(source, name, &*self)
            .map_err(|diagnostics| {
                if diagnostics
                    .iter()
                    .any(crate::compile::Diagnostic::is_stack_exhausted)
                {
                    return self.stack_exhaustion_error();
                }
                let message = diagnostics
                    .first()
                    .map_or("invalid host script".to_owned(), ToString::to_string);
                self.syntax_error_result(&program, &message)
                    .expect_err("specialized host-script syntax errors must throw")
            })?;
        if let (Some(source), Some((line_offset, column_offset))) = (&mut residual.source, origin) {
            source.line_offset = line_offset;
            source.column_offset = column_offset;
        }
        let residual = self.intern_program_atoms(residual);
        Ok(crate::PreparedScript { program: Rc::new(residual) })
    }

    pub(crate) fn compile_embedding_host_script(
        &mut self, source: &str, name: &str, origin: (u32, u32),
    ) -> Result<crate::CompiledScript, JsError> {
        let prepared = self.prepare_embedding_specialized_script(source, name, Some(origin))?;
        let context = self.embedding_program()?;
        let Some(source) = self.store_dynamic_program_shared(prepared.program.clone()) else {
            return Err(self.type_error(&context, "dynamic program store is full".into()));
        };
        let state = Rc::new(crate::api::CompiledScriptState { program: prepared.program, source });
        self.programs.register_owned_script(&state);
        Ok(crate::CompiledScript { state })
    }

    pub(crate) fn compiled_script_is_current(&self, script: &crate::CompiledScript) -> bool {
        script.state.program.atoms.belongs_to(&self.atom_sequence)
            && self.programs.owns_script(&script.state)
    }

    pub(crate) fn evaluate_embedding_compiled_script(
        &mut self, script: &crate::CompiledScript,
    ) -> Result<Value, JsError> {
        let context = self.embedding_program()?;
        if !self.compiled_script_is_current(script) {
            return Err(self.type_error(&context, "compiled Script belongs to a different runtime state".into()));
        }
        let realm = self.heap.weak_handle(self.realm.globals).expect("live realm global");
        let id = match self.programs.owned_script_instance(script.state.source, realm) {
            Some(id) => id,
            None => {
                let Some(id) = self.store_dynamic_program_shared(script.state.program.clone()) else {
                    return Err(self.type_error(&context, "dynamic program store is full".into()));
                };
                self.programs.bind_script_instance(script.state.source, realm, id);
                id
            }
        };
        self.run_global_eval_program(&context, id)
    }

    pub(crate) fn prepared_script_is_current(&self, prepared: &crate::PreparedScript) -> bool {
        prepared.program.atoms.belongs_to(&self.atom_sequence)
    }

    pub(crate) fn evaluate_embedding_prepared_script(
        &mut self, prepared: &crate::PreparedScript,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        if !self.prepared_script_is_current(prepared) {
            return Err(self.type_error(&program, "prepared Script belongs to a different runtime state".into()));
        }
        let Some(program_id) = self.store_dynamic_program_shared(prepared.program.clone()) else {
            return Err(self.type_error(&program, "dynamic program store is full".into()));
        };
        self.run_global_eval_program(&program, program_id)
    }

    /// Evaluates host source as a global Script of the active realm.
    pub(crate) fn evaluate_embedding_global_script(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
        let result = self.eval_script_source(&program, source, name);
        self.direct_eval = direct_eval;
        result
    }

    fn run_global_eval_program(
        &mut self,
        p: &ResidualProgram,
        program_id: super::ProgramId,
    ) -> Result<Value, JsError> {
        let field_initializer = self.direct_eval && self.in_class_field_initializer(p);
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "dynamic program is unavailable".into()))?;
        let active_program = self.replace_active_program(program_id);
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
        if !direct_eval {
            self.programs.mark_global_scope(program_id);
        }
        let root_function = residual.functions[super::ROOT_FUNCTION_ID as usize].clone();
        let root_scope = self
            .frames
            .last()
            .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID);
        let caller_scope = direct_eval && !root_scope && !root_function.strict;
        let previous_eval_var_program = std::mem::replace(
            &mut self.direct_eval_var_program,
            caller_scope.then_some(program_id),
        );
        let result = (|| {
            if !root_function.strict {
                let global_function_atoms = residual.functions[super::ROOT_FUNCTION_ID as usize]
                    .global_function_atoms
                    .clone();
                for atom in &global_function_atoms {
                    if (!caller_scope && self.realm.global_lexical_declarations.contains(atom))
                        || (direct_eval
                            && self
                                .direct_eval_binding(*atom)
                                .is_some_and(|binding| binding.conflicts_with_var()))
                    {
                        return self.syntax_error_result(
                            p,
                            "eval function declaration conflicts with lexical binding",
                        );
                    }
                    if caller_scope {
                        continue;
                    }
                    let globals = self.realm.globals;
                    let name = self.atom_name(*atom).to_owned();
                    self.check_global_eval_declaration(p, globals, &name, true)?;
                }
                for atom in residual.functions[super::ROOT_FUNCTION_ID as usize]
                    .global_var_atoms
                    .iter()
                    .copied()
                {
                    if caller_scope
                        && self.parameter_eval
                        && self
                            .frames
                            .last()
                            .and_then(|frame| p.functions.get(frame.function as usize))
                            .is_some_and(|function| function.parameter_atoms.contains(&atom))
                    {
                        return self.syntax_error_result(
                            p,
                            "eval var declaration conflicts with parameter binding",
                        );
                    }

                    if global_function_atoms.contains(&atom) {
                        continue;
                    }
                    if (!caller_scope && self.realm.global_lexical_declarations.contains(&atom))
                        || (direct_eval
                            && self
                                .direct_eval_binding(atom)
                                .is_some_and(|binding| binding.conflicts_with_var()))
                    {
                        return self.syntax_error_result(
                            p,
                            "eval var declaration conflicts with lexical binding",
                        );
                    }
                    if caller_scope {
                        continue;
                    }
                    let globals = self.realm.globals;
                    let name = self.atom_name(atom).to_owned();
                    self.check_global_eval_declaration(p, globals, &name, false)?;
                }
                if !caller_scope {
                    let globals = self.realm.globals;
                    for atom in &global_function_atoms {
                        let attributes =
                            self.property_attributes(globals, PropertyKey::string(*atom));
                        if attributes.is_some_and(|attributes| attributes.configurable) {
                            self.define_global_eval_binding(p, *atom, Value::UNDEFINED)?;
                        } else if self.own_property(globals, *atom).is_some() {
                            self.set_property_with_program(p, globals, *atom, Value::UNDEFINED)?;
                        } else {
                            self.define_global_eval_binding(p, *atom, Value::UNDEFINED)?;
                        }
                    }
                    for atom in root_function.global_var_atoms.iter().copied() {
                        if global_function_atoms.contains(&atom) {
                            continue;
                        }
                        if self.own_property(globals, atom).is_none() {
                            self.set_property_with_attributes(
                                globals,
                                atom,
                                Value::UNDEFINED,
                                PropertyAttributes {
                                    writable: true,
                                    enumerable: true,
                                    configurable: true,
                                    accessor: false,
                                    getter: None,
                                    setter: None,
                                },
                            )?;
                        }
                    }
                }
            }
            if caller_scope {
                self.prepare_direct_eval_var_bindings(&root_function.global_var_atoms);
            }
            let root_scope = self
                .frames
                .last()
                .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID);
            let parent = if direct_eval {
                if let Some(frame) = self.frames.len().checked_sub(1) {
                    self.capture_binding_environment(frame)?
                } else {
                    Value::NULL
                }
            } else {
                Value::NULL
            };
            let parent = if direct_eval && (root_scope || field_initializer) {
                let dynamic_bindings = if field_initializer {
                    vec![
                        (self.runtime_atoms.new_target, Value::UNDEFINED),
                        (
                            self.runtime_atoms.lexical_this,
                            self.frames
                                .last()
                                .map_or(self.realm.globals, |frame| frame.this),
                        ),
                    ]
                } else {
                    Vec::new()
                };
                self.heap.alloc(Cell::Environment {
                    parent,
                    function: u32::MAX,
                    slots: Vec::<Value>::new().into_boxed_slice().into(),
                    scope: Box::new(crate::heap::EnvironmentScope {
                        program: None,
                        root_eval_scope: root_scope,
                        binding_site_pc: None,
                        dynamic_bindings: dynamic_bindings.into(),
                        with_objects: Box::default(),
                    }),
                })
            } else {
                parent
            };
            let this = if direct_eval {
                self.frames
                    .last()
                    .map_or(self.realm.globals, |frame| frame.this)
            } else {
                self.realm.globals
            };
            let callee = self.closure(&residual, super::ROOT_FUNCTION_ID, parent)?;
            self.call_eval_closure(&residual, callee, this, direct_eval)
        })();
        self.direct_eval_var_program = previous_eval_var_program;
        self.direct_eval = direct_eval;
        self.set_active_program(active_program);
        result
    }

    pub(super) fn capture_binding_environment(&mut self, frame: usize) -> Result<Value, JsError> {
        let binding_site_pc = self.frames[frame].binding_site_pc.filter(|pc| {
            self.programs
                .get(self.frames[frame].program)
                .is_some_and(|program| {
                    program
                        .functions
                        .get(self.frames[frame].function as usize)
                        .and_then(|function| {
                            function
                                .binding_sites
                                .binary_search_by_key(pc, |site| site.resume_pc)
                                .ok()
                                .map(|index| &function.binding_sites[index])
                        })
                        .is_some_and(|site| !site.bindings.is_empty())
                })
        });
        let environment = self.promote_frame_environment(frame);
        let Some(binding_site_pc) = binding_site_pc else {
            return Ok(environment);
        };
        let slots = self
            .heap
            .clone_environment_slots(environment, &[], &[])
            .ok_or_else(|| JsError("invalid eval scope slots".into()))?;
        let owner = self
            .heap
            .environment_binding_owner(environment)
            .ok_or_else(|| JsError("invalid eval scope owner".into()))?;
        let Some(Cell::Environment {
            parent,
            function,
            scope,
            ..
        }) = self.heap.get(environment)
        else {
            return Err(JsError("invalid eval scope environment".into()));
        };
        let scope = Cell::Environment {
            parent: *parent,
            function: *function,
            slots,
            scope: Box::new(crate::heap::EnvironmentScope {
                program: scope.program,
                root_eval_scope: scope.root_eval_scope,
                binding_site_pc: Some(binding_site_pc),
                dynamic_bindings: crate::heap::EnvironmentBindings::Shared(owner),
                with_objects: scope.with_objects.clone(),
            }),
        };
        Ok(self.heap.alloc(scope))
    }

    fn validate_eval_arguments_context(
        &mut self,
        p: &ResidualProgram,
        source: &str,
    ) -> Result<(), JsError> {
        if self.direct_eval
            && self.in_class_field_initializer(p)
            && crate::Engine::field_eval_references_arguments(source)
        {
            return self
                .syntax_error_result(
                    p,
                    "arguments is not allowed in class field initializer eval",
                )
                .map(drop);
        }
        Ok(())
    }

    fn in_class_field_initializer(&mut self, p: &ResidualProgram) -> bool {
        self.frames
            .last()
            .and_then(|frame| p.functions.get(frame.function as usize))
            .is_some_and(|function| function.class_field_initializer)
    }

    fn direct_eval_function_context(&mut self, p: &ResidualProgram) -> bool {
        if !self.direct_eval {
            return false;
        }
        if self.in_class_field_initializer(p) {
            return true;
        }
        let new_target = self.runtime_atoms.new_target;
        self.frames
            .len()
            .checked_sub(1)
            .is_some_and(|frame| self.dynamic_binding(frame, new_target).is_some())
    }

    fn check_global_eval_declaration(
        &mut self,
        p: &ResidualProgram,
        globals: Value,
        name: &str,
        function: bool,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        if self.own_property(globals, atom).is_none() {
            if self
                .object_data(globals)
                .is_some_and(|object| !object.is_extensible())
            {
                return Err(self.type_error(p, format!("cannot define global {name}")));
            }
            return Ok(());
        }
        let Some(attributes) =
            self.property_attributes(globals, crate::vm::property_key::PropertyKey::string(atom))
        else {
            return Ok(());
        };
        if function && !attributes.permits_global_function_declaration() {
            return Err(self.type_error(p, format!("cannot redefine global {name}")));
        }
        Ok(())
    }

    fn call_eval_closure(
        &mut self,
        program: &ResidualProgram,
        callee: Value,
        this: Value,
        direct: bool,
    ) -> Result<Value, JsError> {
        let (function, environment) = match self.call_target(callee)? {
            CallTarget::User(_, function, environment)
            | CallTarget::NumericUser(_, function, environment) => (function, environment),
            CallTarget::Native(_) => return Err(JsError("eval body is not a user closure".into())),
        };
        self.with_call_roots([callee, this], |vm| {
            vm.call_user(
                program,
                function,
                environment,
                this,
                &[],
                if direct {
                    CallContext::DirectEval(callee)
                } else {
                    CallContext::IndirectEval(callee)
                },
            )
        })
    }

    pub(super) fn private_home_binding_atom(&mut self, private: Atom) -> Atom {
        self.intern_atom(&format!("\0quench:private-home:{private}"))
    }

    fn validate_strict_eval(&mut self, p: &ResidualProgram, source: &str) -> Result<(), JsError> {
        let private_names = if self.direct_eval {
            self.direct_eval_private_names(p)
                .map_or_else(Vec::new, |(_, names)| names)
        } else {
            Vec::new()
        };
        let method_context = self.direct_eval_context(p);
        let context_source = if method_context.is_some_and(|context| context.home_atom.is_some())
            || !private_names.is_empty()
        {
            Some(eval_context_source(
                source,
                &private_names,
                method_context.is_some_and(|context| context.super_calls),
            ))
        } else {
            None
        };
        let in_function = method_context.is_some_and(|context| context.in_function);
        if let Some(error) = crate::Engine::strict_eval_syntax_error(
            context_source.as_deref().unwrap_or(source),
            in_function,
        ) {
            return self
                .syntax_error_result(p, error.strip_prefix("SyntaxError: ").unwrap_or(&error))
                .map(drop);
        }
        Ok(())
    }

    fn direct_eval_private_names(
        &mut self,
        p: &ResidualProgram,
    ) -> Option<(Value, Vec<(String, String)>)> {
        let home = self.eval_home_binding(p)?;
        let names = self
            .object_data(home)
            .into_iter()
            .flat_map(|object| object.private_names())
            .filter_map(|brand| {
                let identity = self.atom_name(brand.name).to_owned();
                private_identity_label(&identity).map(|label| (label, identity))
            })
            .fold(Vec::new(), |mut names, binding| {
                if !names.iter().any(|(label, _)| label == &binding.0) {
                    names.push(binding);
                }
                names
            });
        (!names.is_empty()).then_some((home, names))
    }

    fn direct_eval_context(&mut self, p: &ResidualProgram) -> Option<crate::compile::EvalContext> {
        if !self.direct_eval {
            return None;
        }
        let frame = self.frames.len().checked_sub(1)?;
        let home_atom = p
            .functions
            .get(self.frames[frame].function as usize)?
            .super_home_atom;
        let field_initializer = self.in_class_field_initializer(p);
        let atom = self.runtime_atoms.lexical_this;
        let super_calls = home_atom.is_some()
            && !field_initializer
            && self
                .lexical_this_owner(frame, atom)
                .and_then(|(program, function, _)| {
                    self.programs.get(program).and_then(|program| {
                        program
                            .functions
                            .get(function as usize)
                            .map(|f| f.derived_constructor)
                    })
                })
                .unwrap_or(false);
        Some(crate::compile::EvalContext {
            in_function: self.direct_eval_function_context(p),
            home_atom,
            super_calls,
            field_initializer,
        })
    }

    fn eval_home_binding(&mut self, p: &ResidualProgram) -> Option<Value> {
        let function = self.frames.last()?.function;
        let home_atom = p.functions.get(function as usize)?.super_home_atom?;
        // Use the same binding operation as compiled super access. Its retained
        // metadata resolves captures across both scope views and eval programs.
        self.load_name(p, home_atom, None).ok()
    }

    fn direct_eval_binding(&self, atom: Atom) -> Option<crate::bytecode::EvalBinding> {
        self.name_binding(self.frames.len().checked_sub(1)?, atom)
    }

    fn prepare_direct_eval_var_bindings(&mut self, atoms: &[Atom]) {
        let Some(frame_index) = self.frames.len().checked_sub(1) else {
            return;
        };
        let Some(bindings) = self.own_dynamic_bindings(frame_index) else {
            return;
        };
        let mut additions = atoms
            .iter()
            .copied()
            .filter(|atom| {
                (self.parameter_eval || self.activation_binding_slot(frame_index, *atom).is_none())
                    && !bindings.iter().any(|(candidate, _)| candidate == atom)
            })
            .map(|atom| (atom, Value::UNDEFINED))
            .collect::<Vec<_>>();
        if let Some(bindings) = self.own_dynamic_bindings_mut(frame_index) {
            bindings.append(&mut additions);
        }
    }

    pub(super) fn syntax_error_result(
        &mut self,
        p: &ResidualProgram,
        message: &str,
    ) -> Result<Value, JsError> {
        let text = self.heap.alloc(Cell::String(message.into()));
        let error = self.construct_error_native(p, Native::SyntaxError, &[text])?;
        Err(JsError::thrown(error, format!("SyntaxError: {message}")))
    }

    fn define_global_eval_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        let value_root = self.heap.root(value);
        let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
        let value = self.heap.root_value(value_root).unwrap();
        let result = self.define_property_or_throw(
            p,
            self.realm.globals,
            key,
            PropertyDescriptorRecord::data(value),
        );
        self.heap.release_root(value_root);
        result
    }
}

fn private_identity_label(identity: &str) -> Option<String> {
    let identity = identity.strip_prefix("\0quench:private:")?;
    let (_, label) = identity.rsplit_once(':')?;
    (!label.is_empty()).then(|| label.to_owned())
}
