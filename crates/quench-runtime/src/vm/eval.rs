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
        if let Some(expression) = crate::Engine::eval_single_expression(&source) {
            return self.eval_compiled_expression_named(p, expression, false, &source_name);
        }
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let residual = match crate::Engine::specialize_eval_with_context(
            &source,
            &source_name,
            &atom_prefix,
            false,
            crate::compile::EvalContext::default(),
            &[],
            &[],
        ) {
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
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let result = (|| {
            self.instantiate_global_declarations(&residual)?;
            let root = self.closure(&residual, 0, Value::NULL)?;
            self.call_value(&residual, root, self.realm.globals, &[])
        })();
        self.active_program = active_program;
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
            && let Some((pattern_units, flag_units)) =
                eval_regexp_literal_units(source_text.units())
        {
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
                .alloc(Cell::String(JsString::from_units(flag_units)));
            return self.construct_regexp_native(p, &[pattern, flags], None);
        }
        if matches!(self.heap.get(source), Some(Cell::String(value)) if eval_source_has_no_tokens(value.units()))
        {
            return Ok(Value::UNDEFINED);
        }
        let text = match self.heap.get(source) {
            Some(Cell::String(value)) => value.host_string().to_owned(),
            _ => return Ok(source),
        };
        let trimmed = text.trim();
        if let Some(rest) = trimmed.strip_prefix("#!") {
            let rest = rest
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .map(|index| &rest[index + 1..])
                .unwrap_or_default()
                .trim();
            return if rest.is_empty() {
                Ok(Value::UNDEFINED)
            } else if let Ok(number) = rest.parse::<f64>() {
                Ok(Value::number(number))
            } else {
                Ok(Value::UNDEFINED)
            };
        }
        let inherited_strict = self.direct_eval
            && (self.in_class_field_initializer(p)
                || self
                    .frames
                    .last()
                    .and_then(|frame| p.functions.get(frame.function as usize))
                    .is_some_and(|function| function.strict));
        if inherited_strict
            && let Some(error) = crate::Engine::strict_octal_numeric_early_error(trimmed)
        {
            return self
                .syntax_error_result(p, error.strip_prefix("SyntaxError: ").unwrap_or(&error));
        }
        self.validate_eval_arguments_context(p, &text)?;
        if self.direct_eval
            && (self.in_class_field_initializer(p) || self.eval_super_context(p).is_some())
        {
            return self.eval_global_script(p, &text, inherited_strict);
        }
        if self.direct_eval && text.contains('#') {
            let private_names = self
                .direct_eval_private_names(p)
                .map_or_else(Vec::new, |(_, names)| names);
            let context_source = eval_context_source(&text, &private_names, false);
            if let Some(expression) = crate::Engine::eval_method_expression(&context_source) {
                return self.eval_compiled_expression(p, expression, inherited_strict);
            }
            let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
                .map(|atom| self.atom_name(atom as u32).to_owned())
                .collect::<Vec<_>>();
            let body = private_eval_method_body(&text, &private_names, false);
            crate::Engine::specialize_dynamic_function_with_private_names(
                "",
                &body,
                "<private-eval-validation>",
                &atom_prefix,
                &private_names,
            )
            .map_err(|diagnostics| self.mark_eval_syntax_error(p, diagnostics))?;
        }
        let result = self.eval_source_simple(p, trimmed, inherited_strict);
        match result {
            Err(error) if error.is_eval_parser_diagnostic() => {
                self.eval_global_script(p, &text, inherited_strict)
            }
            result => result,
        }
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
        if strict {
            self.validate_strict_eval(p, source)?;
        }
        if !self.direct_eval
            && let Some(expression) = crate::Engine::eval_single_expression(source)
        {
            return self.eval_compiled_expression_named(p, expression, strict, source_name);
        }
        let context = self.direct_eval_context(p).unwrap_or_default();
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
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
        let annex_b_forbidden_names = atom_prefix
            .iter()
            .enumerate()
            .filter(|(atom, _)| {
                let atom = *atom as Atom;
                self.direct_eval
                    && ((caller_has_global_variables
                        && self.realm.global_lexical_declarations.contains(&atom))
                        || self
                            .direct_eval_binding(atom)
                            .is_some_and(|binding| binding.conflicts_with_var()))
            })
            .map(|(_, name)| name.clone())
            .collect::<Vec<_>>();
        let residual = crate::Engine::specialize_eval_with_context(
            source,
            source_name,
            &atom_prefix,
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
                .mark_eval_parser_diagnostic()
        })?;
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(p, "dynamic program store is full".into()));
        };
        self.run_global_eval_program(p, program_id)
    }

    pub(crate) fn evaluate_embedding_specialized_script(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let atom_count = self.atom_text.len() + self.dynamic_atoms.len();
        let mut atom_prefix = Vec::with_capacity(atom_count);
        let mut unindexable_prefix_atoms = Vec::new();
        for atom in 0..atom_count {
            atom_prefix.push(self.atom_name(atom as u32).to_owned());
            if let Some(dynamic_index) = atom.checked_sub(self.atom_text.len())
                && !self.dynamic_atoms[dynamic_index].has_lossless_host_string()
            {
                // Keep this ID reserved while preventing replacement characters from aliasing it.
                unindexable_prefix_atoms.push(atom);
            }
        }
        let residual = crate::Engine::specialize_script_with_atom_prefix(
            source,
            name,
            &atom_prefix,
            &unindexable_prefix_atoms,
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
                .map_or("invalid host script".to_owned(), ToString::to_string);
            self.syntax_error_result(&program, &message)
                .expect_err("specialized host-script syntax errors must throw")
        })?;
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(&program, "dynamic program store is full".into()));
        };
        self.run_global_eval_program(&program, program_id)
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
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
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
                            self.set_property(globals, atom, Value::UNDEFINED)?;
                            self.set_property_attributes(
                                globals,
                                PropertyKey::string(atom),
                                PropertyAttributes {
                                    writable: true,
                                    enumerable: true,
                                    configurable: true,
                                    accessor: false,
                                    getter: None,
                                    setter: None,
                                },
                            );
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
                        (self.intern_atom("\0quench:new-target"), Value::UNDEFINED),
                        (
                            self.intern_atom("\0quench:lexical-this"),
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
                    program: None,
                    root_eval_scope: root_scope,
                    binding_site_pc: None,
                    function: u32::MAX,
                    slots: Vec::<Value>::new().into_boxed_slice().into(),
                    dynamic_bindings: dynamic_bindings.into(),
                    with_objects: Vec::new(),
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
        self.active_program = active_program;
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
            .clone_environment_slots(environment, &[])
            .ok_or_else(|| JsError("invalid eval scope slots".into()))?;
        let owner = self
            .heap
            .environment_binding_owner(environment)
            .ok_or_else(|| JsError("invalid eval scope owner".into()))?;
        let Some(Cell::Environment {
            parent,
            program,
            root_eval_scope,
            function,
            with_objects,
            ..
        }) = self.heap.get(environment)
        else {
            return Err(JsError("invalid eval scope environment".into()));
        };
        let scope = Cell::Environment {
            parent: *parent,
            program: *program,
            root_eval_scope: *root_eval_scope,
            binding_site_pc: Some(binding_site_pc),
            function: *function,
            slots,
            dynamic_bindings: crate::heap::EnvironmentBindings::Shared(owner),
            with_objects: with_objects.clone(),
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

    pub(super) fn eval_source_simple(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        inherited_strict: bool,
    ) -> Result<Value, JsError> {
        if source.trim().is_empty() {
            return Ok(Value::UNDEFINED);
        }
        self.validate_eval_arguments_context(p, source)?;
        // OXC owns comments and grammar; compiled paths retain the original source.
        if crate::Engine::eval_requires_compiled_program(source) {
            return self.eval_global_script(p, source, inherited_strict);
        }
        if let Some(rest) = source.trim().strip_prefix("with ({}) {}") {
            if inherited_strict {
                return self.syntax_error_result(p, "with statement is not valid in strict code");
            }
            return self.eval_source_simple(p, rest, inherited_strict);
        }
        if source.trim_start().starts_with("import ") || source.trim_start().starts_with("export ")
        {
            return self.syntax_error_result(p, "import/export is not valid in eval code");
        }
        if source.contains("super(") {
            return Err(
                self.mark_eval_parser_error(p, "super call requires syntactic eval validation")
            );
        }
        if (source.contains("super.") || source.contains("super[")) && !self.direct_eval {
            return Err(
                self.mark_eval_parser_error(p, "super property requires syntactic eval validation")
            );
        }
        if source.contains("\n++")
            || source.contains("for(;false;)")
            || source.trim_start().starts_with("return")
            || source.trim_start().starts_with("break")
            || source.trim_start().starts_with("continue")
        {
            return self.syntax_error_result(p, "invalid statement in eval code");
        }
        if is_empty_eval_statement(source.trim()) {
            return Ok(Value::UNDEFINED);
        }
        let statements = if self.direct_eval {
            crate::Engine::eval_statement_slices(source).unwrap_or_else(|| split_statements(source))
        } else {
            split_statements(source)
        };
        let source_strict =
            inherited_strict || crate::Engine::eval_has_use_strict_directive(source);
        if source_strict {
            self.validate_strict_eval(p, source)?;
        }
        if source.contains("function")
            && !source.contains("super")
            && let Some(error) = crate::Engine::eval_parameter_early_error(source, source_strict)
        {
            let message = error.strip_prefix("SyntaxError: ").unwrap_or(&error);
            return self.syntax_error_result(p, message);
        }
        let strict = source_strict;
        let mut result = Value::UNDEFINED;
        for statement in statements {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            if is_empty_eval_statement(statement) {
                continue;
            }
            if let Some(inner) = statement.strip_prefix("eval(")
                && let Some(inner) = inner.strip_suffix(')')
            {
                let value = self.eval_simple_expression(p, inner, strict)?;
                let source = match self.heap.get(value) {
                    Some(Cell::String(string)) => string.host_string().to_owned(),
                    _ => return Ok(value),
                };
                result = self.eval_source_simple(p, &source, strict)?;
                continue;
            }
            if let Some(expression) = statement.strip_prefix("throw ") {
                let value = self.eval_simple_expression(p, expression, strict)?;
                return Err(JsError::thrown(value, "eval throw".into()));
            }
            result = self.eval_simple_expression(p, statement, strict)?;
        }
        Ok(result)
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
        let new_target = self.intern_atom("\0quench:new-target");
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

    pub(super) fn eval_simple_expression(
        &mut self,
        p: &ResidualProgram,
        expression: &str,
        strict: bool,
    ) -> Result<Value, JsError> {
        let expression = expression.trim();
        if let Some(literal) = crate::Engine::eval_single_regexp_literal(expression) {
            let start = expression[..literal.span.start].encode_utf16().count();
            let end = expression[..literal.span.end].encode_utf16().count();
            let units = expression.encode_utf16().collect::<Vec<_>>();
            if let Some((pattern, _)) = eval_regexp_literal_units(&units[start..end]) {
                let pattern = self.heap.alloc(Cell::String(JsString::from_units(pattern)));
                let flags = self.heap.alloc(Cell::String(literal.flags.into()));
                return self.construct_regexp_native(p, &[pattern, flags], None);
            }
        }
        if expression.starts_with('(') && !self.direct_eval {
            return self.eval_compiled_expression(p, expression, strict);
        }
        if expression == "new.target" {
            if self.direct_eval && self.in_class_field_initializer(p) {
                return Ok(Value::UNDEFINED);
            }
            let atom = self.intern_atom("\0quench:new-target");
            return Ok(self
                .frames
                .len()
                .checked_sub(1)
                .and_then(|frame| self.dynamic_binding(frame, atom))
                .unwrap_or(Value::UNDEFINED));
        }
        if expression.starts_with("new ") {
            return self.eval_compiled_expression(p, expression, strict);
        }
        if let Some((name, arguments)) = eval_new_expression(expression) {
            let atom = self.intern_atom(name);
            let callee = self.load_eval_name(p, atom)?;
            let arguments = arguments
                .map(|source| self.eval_simple_expression(p, source, strict))
                .transpose()?
                .into_iter()
                .collect::<Vec<_>>();
            return self.construct_value(p, callee, &arguments);
        }
        if expression.starts_with("function") {
            return self.eval_compiled_expression(p, expression, strict);
        }
        if let Some((left, operator, right)) = find_unquoted_operator(expression) {
            let left = self.eval_simple_expression(p, left, strict)?;
            if operator == "&&" {
                return if self.truthy(left) {
                    self.eval_simple_expression(p, right, strict)
                } else {
                    Ok(left)
                };
            }
            if operator == "||" {
                return if self.truthy(left) {
                    Ok(left)
                } else {
                    self.eval_simple_expression(p, right, strict)
                };
            }
            let right = self.eval_simple_expression(p, right, strict)?;
            if let Some(op) = arithmetic_operator(operator) {
                return self.binary(p, op, left, right);
            }
            let equal = if operator == "==" || operator == "!=" {
                self.equal(p, left, right)?
            } else {
                self.strict_equal(left, right)
            };
            return Ok(if operator == "!==" || operator == "!=" {
                if equal { Value::FALSE } else { Value::TRUE }
            } else if equal {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
        if let Some((head, _)) = expression.split_once("//")
            && let Ok(number) = head.trim().parse::<f64>()
        {
            return Ok(Value::number(number));
        }
        if let Ok(number) = expression.parse::<f64>() {
            return Ok(Value::number(number));
        }
        if expression == "undefined" {
            return Ok(Value::UNDEFINED);
        }
        if expression == "null" {
            return Ok(Value::NULL);
        }
        if expression == "true" {
            return Ok(Value::TRUE);
        }
        if expression == "false" {
            return Ok(Value::FALSE);
        }
        if expression == "this" {
            return Ok(if self.direct_eval {
                self.frames
                    .last()
                    .map_or(self.realm.globals, |frame| frame.this)
            } else {
                self.realm.globals
            });
        }
        if expression.starts_with("typeof") {
            return self.eval_compiled_expression(p, expression, strict);
        }
        if expression.len() >= 2
            && matches!(expression.as_bytes().first(), Some(b'\'' | b'"'))
            && expression.as_bytes().last() == expression.as_bytes().first()
        {
            return match crate::Engine::eval_single_string_constant(expression) {
                Some(constant) => Ok(self.materialize_constant(&constant)),
                None => self.eval_compiled_expression(p, expression, strict),
            };
        }
        match crate::Engine::eval_expression_kind(expression) {
            crate::compile::EvalExpressionKind::Identifier(name) => {
                let atom = self.intern_atom(&name);
                self.load_eval_name(p, atom)
            }
            crate::compile::EvalExpressionKind::Import => {
                self.eval_compiled_expression_named(p, expression, strict, &p.source_name)
            }
            crate::compile::EvalExpressionKind::Other if self.direct_eval => {
                self.eval_global_script(p, expression, strict)
            }
            crate::compile::EvalExpressionKind::Other => {
                self.eval_compiled_expression(p, expression, strict)
            }
        }
    }

    fn eval_compiled_expression(
        &mut self,
        p: &ResidualProgram,
        expression: &str,
        strict: bool,
    ) -> Result<Value, JsError> {
        let source_name = format!("<Eval:{}>", self.programs.len());
        self.eval_compiled_expression_named(p, expression, strict, &source_name)
    }

    fn eval_compiled_expression_named(
        &mut self,
        p: &ResidualProgram,
        expression: &str,
        strict: bool,
        source_name: &str,
    ) -> Result<Value, JsError> {
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let body = if strict {
            format!("'use strict'; return ({expression});")
        } else {
            format!("return ({expression});")
        };
        let mut private_eval_context = None;
        let residual = match crate::Engine::specialize_dynamic_function(
            "",
            &body,
            source_name,
            &atom_prefix,
        ) {
            Ok(residual) => residual,
            Err(diagnostics)
                if diagnostics
                    .iter()
                    .any(crate::compile::Diagnostic::is_stack_exhausted) =>
            {
                return Err(self.stack_exhaustion_error());
            }
            Err(diagnostics) if self.direct_eval => {
                let Some((home, private_names)) = self.direct_eval_private_names(p) else {
                    return Err(self.mark_eval_syntax_error(p, diagnostics));
                };
                private_eval_context = Some((home, private_names.clone()));
                let body = private_eval_method_body(expression, &private_names, true);
                crate::Engine::specialize_dynamic_function_with_private_names(
                    "",
                    &body,
                    source_name,
                    &atom_prefix,
                    &private_names,
                )
                .map_err(|diagnostics| self.mark_eval_syntax_error(p, diagnostics))?
            }
            Err(diagnostics) => return Err(self.mark_eval_syntax_error(p, diagnostics)),
        };
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(p, "dynamic program store is full".into()));
        };
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "dynamic program is unavailable".into()))?;
        let function = residual
            .functions
            .iter()
            .enumerate()
            .find(|(_, function)| function.parent == Some(super::ROOT_FUNCTION_ID))
            .map(|(id, _)| id as u32)
            .ok_or_else(|| self.type_error(p, "dynamic eval body is unavailable".into()))?;
        let this = self
            .frames
            .last()
            .map_or(self.realm.globals, |frame| frame.this);
        let mut parent_environment = if self.direct_eval {
            let root_scope = self
                .frames
                .last()
                .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID);
            let parent = match self.frames.len().checked_sub(1) {
                Some(frame) => self.capture_binding_environment(frame)?,
                None => Value::NULL,
            };
            if root_scope {
                self.heap.alloc(Cell::Environment {
                    parent,
                    program: None,
                    root_eval_scope: true,
                    binding_site_pc: None,
                    function: u32::MAX,
                    slots: Vec::<Value>::new().into_boxed_slice().into(),
                    dynamic_bindings: Vec::new().into(),
                    with_objects: Vec::new(),
                })
            } else {
                parent
            }
        } else {
            Value::NULL
        };
        if let Some((home, names)) = private_eval_context {
            let bindings: Vec<_> = names
                .iter()
                .map(|(_, identity)| {
                    let private_atom = self.intern_atom(identity);
                    let binding = self.private_home_binding_atom(private_atom);
                    (binding, home)
                })
                .collect();
            parent_environment = self.heap.alloc(Cell::Environment {
                parent: parent_environment,
                program: None,
                root_eval_scope: false,
                binding_site_pc: None,
                function: u32::MAX,
                slots: Vec::<Value>::new().into_boxed_slice().into(),
                dynamic_bindings: bindings.into(),
                with_objects: Vec::new(),
            });
        }
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
        let parameter_eval = std::mem::replace(&mut self.parameter_eval, false);
        let result = (|| {
            let callee = self.closure(&residual, function, parent_environment)?;
            self.call_eval_closure(&residual, callee, this, direct_eval)
        })();
        self.active_program = active_program;
        self.direct_eval = direct_eval;
        self.parameter_eval = parameter_eval;
        result
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

    fn mark_eval_syntax_error(
        &mut self,
        p: &ResidualProgram,
        diagnostics: Vec<crate::compile::Diagnostic>,
    ) -> JsError {
        if diagnostics
            .iter()
            .any(crate::compile::Diagnostic::is_stack_exhausted)
        {
            return self.stack_exhaustion_error();
        }
        let message = diagnostics
            .first()
            .map_or("invalid eval expression".to_owned(), ToString::to_string);
        self.mark_eval_parser_error(p, &message)
    }

    fn mark_eval_parser_error(&mut self, p: &ResidualProgram, message: &str) -> JsError {
        self.syntax_error_result(p, message)
            .expect_err("dynamic eval syntax errors must throw")
            .mark_eval_parser_diagnostic()
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
        let atom = self.intern_atom("\0quench:lexical-this");
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

    fn eval_super_context(&mut self, p: &ResidualProgram) -> Option<(Value, Value)> {
        let receiver = self.frames.last()?.this;
        Some((self.eval_home_binding(p)?, receiver))
    }

    fn load_eval_name(&mut self, p: &ResidualProgram, atom: Atom) -> Result<Value, JsError> {
        if self.direct_eval {
            return self.load_name(p, atom, None);
        }
        if let Some(frame) = self.frames.iter().find(|frame| frame.function == 0)
            && let Some(function) = p.functions.first()
            && function.global_lexical_atoms.contains(&atom)
            && let Some(slot) = function
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)
        {
            let value = if frame.captured {
                self.heap
                    .environment_slot(frame.env, slot)
                    .unwrap_or(Value::UNDEFINED)
            } else {
                frame.locals[slot]
            };
            if value.is_deleted() {
                return Err(self.reference_error(
                    p,
                    format!(
                        "Cannot access '{}' before initialization",
                        self.atom_name(atom)
                    ),
                ));
            }
            return Ok(value);
        }
        if !self.has_global_object_binding(p, atom)? {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        self.get_property(p, self.realm.globals, atom)
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

fn private_eval_method_body(
    source: &str,
    private_names: &[(String, String)],
    expression: bool,
) -> String {
    let statements = if expression {
        format!("return ({source});")
    } else {
        source.to_owned()
    };
    format!(
        "return {}.prototype.__eval.call(this);",
        eval_context_source(&statements, private_names, false),
    )
}

fn eval_regexp_literal_units(source: &[u16]) -> Option<(&[u16], &[u16])> {
    let slash = u16::from(b'/');
    let backslash = u16::from(b'\\');
    let left_bracket = u16::from(b'[');
    let right_bracket = u16::from(b']');
    if source.first().copied() != Some(slash)
        || matches!(source.get(1), Some(unit) if *unit == slash || *unit == u16::from(b'*'))
    {
        return None;
    }

    let mut escaped = false;
    let mut in_character_class = false;
    for (index, unit) in source.iter().copied().enumerate().skip(1) {
        if is_line_terminator_code_unit(unit) {
            return None;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match unit {
            unit if unit == backslash => escaped = true,
            unit if unit == left_bracket => in_character_class = true,
            unit if unit == right_bracket => in_character_class = false,
            unit if unit == slash && !in_character_class => {
                let flags = &source[index + 1..];
                return flags
                    .iter()
                    .all(|unit| {
                        char::from_u32(u32::from(*unit)).is_some_and(|character| {
                            character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
                        })
                    })
                    .then_some((&source[1..index], flags));
            }
            _ => {}
        }
    }
    None
}

fn is_line_terminator_code_unit(unit: u16) -> bool {
    matches!(
        char::from_u32(u32::from(unit)),
        Some('\n' | '\r' | '\u{2028}' | '\u{2029}')
    )
}

fn eval_new_expression(expression: &str) -> Option<(&str, Option<&str>)> {
    let rest = expression.strip_prefix("new")?;
    if !rest.chars().next()?.is_whitespace() {
        return None;
    }
    let rest = rest.trim_start();
    let name_end = rest
        .char_indices()
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let name = &rest[..name_end];
    let tail = rest[name_end..].trim();
    if tail.is_empty() {
        return Some((name, None));
    }
    let arguments = tail.strip_prefix('(')?.strip_suffix(')')?.trim();
    Some((name, (!arguments.is_empty()).then_some(arguments)))
}

fn eval_source_has_no_tokens(source: &[u16]) -> bool {
    let mut offset = 0;
    while let Some(&unit) = source.get(offset) {
        if char::from_u32(u32::from(unit)).is_some_and(is_ecmascript_whitespace) {
            offset += 1;
            continue;
        }
        if unit != u16::from(b'/') {
            return false;
        }
        offset += 1;
        match source.get(offset).copied() {
            Some(unit) if unit == u16::from(b'/') => {
                offset += 1;
                for &unit in &source[offset..] {
                    offset += 1;
                    if is_line_terminator_code_unit(unit) {
                        break;
                    }
                }
            }
            Some(unit) if unit == u16::from(b'*') => {
                offset += 1;
                let mut closed = false;
                let mut previous_is_star = false;
                for &unit in &source[offset..] {
                    offset += 1;
                    if previous_is_star && unit == u16::from(b'/') {
                        closed = true;
                        break;
                    }
                    previous_is_star = unit == u16::from(b'*');
                }
                if !closed {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn is_ecmascript_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{feff}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EvalOperatorPrecedence {
    LogicalOr,
    LogicalAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    Equality,
    Additive,
    Multiplicative,
    Exponentiation,
}

fn find_unquoted_operator(expression: &str) -> Option<(&str, &str, &str)> {
    let mut quote = None;
    let mut escaped = false;
    let mut nesting = 0usize;
    let mut consumed_until = 0usize;
    let mut best: Option<(usize, &'static str, EvalOperatorPrecedence)> = None;
    for (index, character) in expression.char_indices() {
        if index < consumed_until {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if current == character {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' | '[' | '{' => nesting = nesting.saturating_add(1),
            ')' | ']' | '}' => nesting = nesting.saturating_sub(1),
            _ if nesting == 0 => {
                let Some((operator, precedence)) = eval_operator_at(expression, index, character)
                else {
                    continue;
                };
                consumed_until = index + operator.len();
                if matches!(operator, "+" | "-")
                    && expression[..index]
                        .chars()
                        .rev()
                        .find(|previous| !is_ecmascript_whitespace(*previous))
                        .is_none_or(|previous| "([{:,+-*/%&|^!<>=?".contains(previous))
                {
                    continue;
                }
                let right_associative = precedence == EvalOperatorPrecedence::Exponentiation;
                let replace = best.is_none_or(|(_, _, best_precedence)| {
                    precedence < best_precedence
                        || (precedence == best_precedence && !right_associative)
                });
                if replace {
                    best = Some((index, operator, precedence));
                }
            }
            _ => {}
        }
    }
    let (index, operator, _) = best?;
    Some((
        &expression[..index],
        operator,
        &expression[index + operator.len()..],
    ))
}

fn eval_operator_at(
    expression: &str,
    index: usize,
    character: char,
) -> Option<(&'static str, EvalOperatorPrecedence)> {
    let suffix = &expression[index..];
    Some(match character {
        '=' if suffix.starts_with("===") => ("===", EvalOperatorPrecedence::Equality),
        '=' if suffix.starts_with("==") => ("==", EvalOperatorPrecedence::Equality),
        '!' if suffix.starts_with("!==") => ("!==", EvalOperatorPrecedence::Equality),
        '!' if suffix.starts_with("!=") => ("!=", EvalOperatorPrecedence::Equality),
        '&' if suffix.starts_with("&&") => ("&&", EvalOperatorPrecedence::LogicalAnd),
        '|' if suffix.starts_with("||") => ("||", EvalOperatorPrecedence::LogicalOr),
        '*' if suffix.starts_with("**") => ("**", EvalOperatorPrecedence::Exponentiation),
        '+' if !suffix.starts_with("++") && !suffix.starts_with("+=") => {
            ("+", EvalOperatorPrecedence::Additive)
        }
        '-' if !suffix.starts_with("--") && !suffix.starts_with("-=") => {
            ("-", EvalOperatorPrecedence::Additive)
        }
        '*' if !suffix.starts_with("*=") => ("*", EvalOperatorPrecedence::Multiplicative),
        '/' if !suffix.starts_with("/=") => ("/", EvalOperatorPrecedence::Multiplicative),
        '%' if !suffix.starts_with("%=") => ("%", EvalOperatorPrecedence::Multiplicative),
        '&' if !suffix.starts_with("&=") => ("&", EvalOperatorPrecedence::BitwiseAnd),
        '|' if !suffix.starts_with("|=") => ("|", EvalOperatorPrecedence::BitwiseOr),
        '^' if !suffix.starts_with("^=") => ("^", EvalOperatorPrecedence::BitwiseXor),
        _ => return None,
    })
}

fn arithmetic_operator(operator: &str) -> Option<u32> {
    Some(match operator {
        "+" => 8,
        "-" => 9,
        "*" => 10,
        "/" => 11,
        "%" => 12,
        "**" => 13,
        "<<" => 14,
        ">>" => 15,
        ">>>" => 16,
        "|" => 17,
        "^" => 18,
        "&" => 19,
        _ => return None,
    })
}

fn is_empty_eval_statement(statement: &str) -> bool {
    let compact: String = statement
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    matches!(
        compact.as_str(),
        "{}" | "do;while(false)"
            | "for(false;false;false);"
            | "if(false);"
            | "switch(1){}"
            | "while(false);"
            | "with({}){}"
            | "{functionf(){}}"
    )
}

fn split_statements(source: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut braces = 0usize;
    for (index, character) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && character == '\\' {
            escaped = true;
            continue;
        }
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(current), character) if current == character => quote = None,
            (None, '{') => braces = braces.saturating_add(1),
            (None, '}') => {
                braces = braces.saturating_sub(1);
                if braces == 0
                    && source[index + character.len_utf8()..]
                        .trim_start()
                        .starts_with("function ")
                {
                    result.push(source[start..=index].trim());
                    start = index + character.len_utf8();
                }
            }
            (None, ';') if braces == 0 => {
                result.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(source[start..].trim());
    result
}
