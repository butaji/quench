use std::borrow::Cow;

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

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
        let residual = match crate::Engine::specialize_eval_unspecialized_with_atom_prefix(
            &source,
            &source_name,
            &atom_prefix,
            false,
            false,
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
            && let Some((pattern_units, flag_units)) = eval_regexp_literal_units(source_text.units())
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
            let flags = self.heap.alloc(Cell::String(JsString::from_units(flag_units)));
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
        let text = strip_eval_comments(&text);
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
            && self
                .frames
                .last()
                .and_then(|frame| p.functions.get(frame.function as usize))
                .is_some_and(|function| function.strict);
        if inherited_strict
            && let Some(error) = crate::Engine::strict_octal_numeric_early_error(trimmed)
        {
            return self
                .syntax_error_result(p, error.strip_prefix("SyntaxError: ").unwrap_or(&error));
        }
        if self.direct_eval && text.contains('#') {
            let private_names = self
                .direct_eval_private_names(p)
                .map_or_else(Vec::new, |(_, names)| names);
            let context_source = eval_method_context_source(&text, &private_names);
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
        if strict {
            self.validate_strict_eval(p, source)?;
        }
        if !self.direct_eval
            && let Some(expression) = crate::Engine::eval_single_expression(source)
        {
            return self.eval_compiled_expression_named(p, expression, strict, &source_name);
        }
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let residual = crate::Engine::specialize_eval_unspecialized_with_atom_prefix(
            source,
            &source_name,
            &atom_prefix,
            strict,
            self.direct_eval_function_context(p),
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
                                .direct_eval_lexical_binding(p, *atom)
                                .is_some_and(|binding| !binding.catch_parameter))
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
                                .direct_eval_lexical_binding(p, atom)
                                .is_some_and(|binding| !binding.catch_parameter))
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
                self.frames
                    .len()
                    .checked_sub(1)
                    .map_or(Value::NULL, |frame| self.promote_frame_environment(frame))
            } else {
                Value::NULL
            };
            let parent = if direct_eval && (root_scope || field_initializer) {
                let dynamic_bindings = if field_initializer {
                    vec![(self.intern_atom("\0rqj:new-target"), Value::UNDEFINED)]
                } else {
                    Vec::new()
                };
                self.heap.alloc(Cell::Environment {
                    parent,
                    program: None,
                    root_eval_scope: root_scope,
                    function: u32::MAX,
                    slots: Box::new([]),
                    dynamic_bindings,
                    with_objects: Vec::new(),
                })
            } else {
                parent
            };
            let root = self.closure(&residual, super::ROOT_FUNCTION_ID, parent)?;
            let this = if direct_eval {
                self.frames
                    .last()
                    .map_or(self.realm.globals, |frame| frame.this)
            } else {
                self.realm.globals
            };
            self.call_value(&residual, root, this, &[])
        })();
        self.direct_eval_var_program = previous_eval_var_program;
        self.direct_eval = direct_eval;
        self.active_program = active_program;
        result
    }

    pub(super) fn eval_source_simple(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        inherited_strict: bool,
    ) -> Result<Value, JsError> {
        let binding_count = self
            .frames
            .last()
            .map_or(0, |frame| frame.dynamic_bindings.len());
        let eval_source_strict =
            inherited_strict || crate::Engine::eval_has_use_strict_directive(source);
        let eval_var_names = if self.direct_eval && !eval_source_strict {
            crate::Engine::eval_var_names(source).unwrap_or_default()
        } else {
            Vec::new()
        };
        let check_lexical_conflicts = self.direct_eval && !eval_source_strict;
        let mut retained_var_bindings = Vec::with_capacity(eval_var_names.len());
        let mut lexical_conflict = false;
        for name in eval_var_names {
            let atom = self.intern_atom(&name);
            lexical_conflict |= check_lexical_conflicts
                && (self.realm.global_lexical_declarations.contains(&atom)
                    || self
                        .direct_eval_lexical_binding(p, atom)
                        .is_some_and(|binding| !binding.catch_parameter)
                    || (!self.parameter_eval
                        && self
                            .frames
                            .last()
                            .and_then(|frame| p.functions.get(frame.function as usize))
                            .is_some_and(|function| function.lexical_atoms.contains(&atom))));
            retained_var_bindings.push(atom);
        }
        let result = if lexical_conflict {
            self.syntax_error_result(p, "eval var declaration conflicts with lexical binding")
        } else {
            self.eval_source_simple_body(p, source, inherited_strict)
        };
        if let Some(frame) = self.frames.last_mut() {
            let mut index = 0;
            frame.dynamic_bindings.retain(|(atom, _)| {
                let keep = index < binding_count || retained_var_bindings.contains(atom);
                index += 1;
                keep
            });
        }
        self.sync_dynamic_bindings();
        result
    }

    fn eval_source_simple_body(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        inherited_strict: bool,
    ) -> Result<Value, JsError> {
        if source.trim().is_empty() {
            return Ok(Value::UNDEFINED);
        }
        if self.direct_eval
            && contains_eval_identifier(source, "arguments")
            && (self.in_class_field_initializer(p)
                || self
                    .frames
                    .last()
                    .and_then(|frame| p.functions.get(frame.function as usize))
                    .is_some_and(|function| function.class_field_initializer))
        {
            return self.syntax_error_result(
                p,
                "arguments is not allowed in class field initializer eval",
            );
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
        if crate::Engine::eval_requires_compiled_program(source) {
            return self.eval_global_script(p, source, inherited_strict);
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
        let mut result = crate::Engine::eval_directives(source)
            .and_then(|directives| directives.last().cloned())
            .map(|directive| self.heap.alloc(Cell::String(directive.into())))
            .unwrap_or(Value::UNDEFINED);
        for statement in statements {
            let statement = statement.trim();
            if statement.is_empty() || is_use_strict(statement) {
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
            if let Some((name, expression)) = split_assignment(statement) {
                if strict && is_strict_reserved(name) {
                    return self.syntax_error_result(p, "reserved assignment in strict eval");
                }
                let atom = self.intern_atom(name);
                let value = self.eval_simple_expression(p, expression, strict)?;
                self.store_eval_name(p, atom, value, strict, false)?;
                if !self.direct_eval && !strict {
                    self.store_frame_local(p, atom, value);
                    self.store_eval_outer_local(p, atom, value);
                } else if self.direct_eval && !strict {
                    self.store_eval_outer_local(p, atom, value);
                }
                result = value;
                continue;
            }
            result = self.eval_simple_expression(p, statement, strict)?;
        }
        Ok(result)
    }

    fn in_class_field_initializer(&mut self, p: &ResidualProgram) -> bool {
        p.atoms.iter().enumerate().any(|(index, name)| {
            if !name.starts_with("\0rqj:class-field-eval:") {
                return false;
            }
            let atom = index as Atom;
            self.load_eval_capture_atom(p, atom)
                .or_else(|| self.load_eval_frame_local(p, atom))
                == Some(Value::TRUE)
        })
    }

    fn direct_eval_function_context(&mut self, p: &ResidualProgram) -> bool {
        if !self.direct_eval {
            return false;
        }
        if self.in_class_field_initializer(p) {
            return true;
        }
        let new_target = self.intern_atom("\0rqj:new-target");
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
        if function && !attributes.configurable && (!attributes.writable || attributes.accessor) {
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
            let atom = self.intern_atom("\0rqj:new-target");
            return Ok(self
                .frames
                .last()
                .and_then(|frame| {
                    frame
                        .dynamic_bindings
                        .iter()
                        .rev()
                        .find_map(|(candidate, value)| (*candidate == atom).then_some(*value))
                })
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
        if let Some(key) = eval_super_property_key(expression) {
            if !self.direct_eval {
                return self.syntax_error_result(p, "super property is not valid in eval code");
            }
            let Some((home, receiver)) = self.eval_super_context(p) else {
                return self.syntax_error_result(p, "super property is not valid in eval code");
            };
            let base = self
                .object_data(home)
                .map_or(Value::NULL, |object| object.proto);
            let key = self.intern_atom(&key);
            return self.get_property_with_receiver(p, base, key, receiver);
        }
        if let Some(body) = expression.strip_prefix("() =>")
            && let Some(key) = eval_super_property_key(body.trim().trim_end_matches(';').trim())
        {
            if !self.direct_eval {
                return self.syntax_error_result(p, "super property is not valid in eval code");
            }
            let Some((home, receiver)) = self.eval_super_context(p) else {
                return self.syntax_error_result(p, "super property is not valid in eval code");
            };
            let key = Value::number(self.intern_atom(&key) as f64);
            let marker = self
                .heap
                .alloc(Cell::String("\0rqj:eval-super-arrow".into()));
            let env = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(vec![marker, key, home, receiver]),
            });
            return Ok(self.native_with_env(Native::DynamicFunction, env));
        }
        if let Some(rest) = expression.strip_prefix("typeof") {
            let separated = rest
                .chars()
                .next()
                .is_some_and(is_ecmascript_whitespace)
                .then(|| rest.trim_start_matches(is_ecmascript_whitespace));
            let syntax = separated.unwrap_or(rest);
            let parenthesized = syntax
                .strip_prefix('(')
                .and_then(|operand| operand.strip_suffix(')'));
            let operand = parenthesized
                .or(separated)
                .filter(|operand| !operand.trim().is_empty());
            if let Some(operand) = operand {
                let is_identifier =
                    operand.chars().next().is_some_and(|character| {
                        character == '_' || character == '$' || character.is_ascii_alphabetic()
                    }) && operand.chars().all(|character| {
                        character == '_' || character == '$' || character.is_ascii_alphanumeric()
                    }) && !matches!(operand, "undefined" | "null" | "true" | "false" | "this");
                let value = if is_identifier {
                    let atom = self.intern_atom(operand);
                    self.load_eval_name(p, atom)?
                } else {
                    self.eval_simple_expression(p, operand, strict)?
                };
                return Ok(self.typeof_value(value));
            }
        }
        if expression.len() >= 2
            && matches!(expression.as_bytes().first(), Some(b'\'' | b'"'))
            && expression.as_bytes().last() == expression.as_bytes().first()
        {
            let text = &expression[1..expression.len() - 1];
            return Ok(self.heap.alloc(Cell::String(
                text.replace("\\'", "'").replace("\\\"", "\"").into(),
            )));
        }
        if simple_eval_call(expression).is_some_and(|(name, _)| name == "import") {
            return self.eval_compiled_expression_named(p, expression, strict, &p.source_name);
        }
        if let Some((name, argument_text)) = simple_eval_call(expression) {
            let atom = self.intern_atom(name);
            let callee = self.load_eval_name(p, atom)?;
            let arguments = if argument_text.trim().is_empty() {
                Vec::new()
            } else {
                vec![self.eval_simple_expression(p, argument_text, strict)?]
            };
            let previous_direct_eval = self.direct_eval;
            let previous_parameter_eval = self.parameter_eval;
            self.direct_eval = false;
            self.parameter_eval = false;
            let result = self.call_value(p, callee, Value::UNDEFINED, &arguments);
            self.direct_eval = previous_direct_eval;
            self.parameter_eval = previous_parameter_eval;
            return result;
        }
        let atom = self.intern_atom(expression);
        match self.load_eval_name(p, atom) {
            Ok(value) => Ok(value),
            Err(_) if self.direct_eval => self.eval_global_script(p, expression, strict),
            Err(_) => self.eval_compiled_expression(p, expression, strict),
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
            .find(|(_, function)| {
                function.parent == Some(0)
                    && function
                        .name
                        .is_some_and(|name| &residual.atoms[name as usize] == "anonymous")
            })
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
            let parent = self
                .frames
                .len()
                .checked_sub(1)
                .map_or(Value::NULL, |frame| self.promote_frame_environment(frame));
            if root_scope {
                self.heap.alloc(Cell::Environment {
                    parent,
                    program: None,
                    root_eval_scope: true,
                    function: u32::MAX,
                    slots: Box::new([]),
                    dynamic_bindings: Vec::new(),
                    with_objects: Vec::new(),
                })
            } else {
                parent
            }
        } else {
            Value::NULL
        };
        if let Some((home, names)) = private_eval_context {
            let bindings = names
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
                function: u32::MAX,
                slots: Box::new([]),
                dynamic_bindings: bindings,
                with_objects: Vec::new(),
            });
        }
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
        let parameter_eval = std::mem::replace(&mut self.parameter_eval, false);
        let result = (|| {
            let closure = self.closure(&residual, function, parent_environment)?;
            self.call_value(p, closure, this, &[])
        })();
        self.active_program = active_program;
        self.direct_eval = direct_eval;
        self.parameter_eval = parameter_eval;
        result
    }

    pub(super) fn private_home_binding_atom(&mut self, private: Atom) -> Atom {
        self.intern_atom(&format!("\0rqj:private-home:{private}"))
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
        let super_property = self.direct_eval && self.eval_super_context(p).is_some();
        let context_source = if super_property || !private_names.is_empty() {
            Some(eval_method_context_source(source, &private_names))
        } else {
            None
        };
        if let Some(error) = crate::Engine::strict_eval_syntax_error(
            context_source.as_deref().unwrap_or(source),
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
        let Some(frame) = self.frames.last() else {
            return None;
        };
        let Some(home_atom) = p
            .functions
            .get(frame.function as usize)
            .and_then(|function| function.super_home_atom)
        else {
            return None;
        };
        let home = self
            .load_eval_capture_atom(p, home_atom)
            .or_else(|| self.load_eval_frame_local(p, home_atom));
        let Some(home) = home else {
            return None;
        };
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

    fn eval_super_context(&mut self, p: &ResidualProgram) -> Option<(Value, Value)> {
        let frame = self.frames.last()?;
        let function = p.functions.get(frame.function as usize)?;
        let home_atom = function.super_home_atom?;
        let receiver = frame.this;
        let home = self
            .load_eval_capture_atom(p, home_atom)
            .or_else(|| self.load_eval_frame_local(p, home_atom))?;
        Some((home, receiver))
    }

    pub(super) fn call_eval_super_arrow(
        &mut self,
        p: &ResidualProgram,
        env: Value,
    ) -> Result<Option<Value>, JsError> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(env) else {
            return Ok(None);
        };
        let elements = elements.as_ref().clone();
        if elements.len() != 4
            || !matches!(self.heap.get(elements[0]), Some(Cell::String(value)) if value.host_string() == "\0rqj:eval-super-arrow")
        {
            return Ok(None);
        }
        let Some(atom) = elements[1].as_number().map(|value| value as Atom) else {
            return Err(JsError("invalid eval super-arrow property key".into()));
        };
        let home = elements[2];
        let receiver = elements[3];
        let base = self
            .object_data(home)
            .map_or(Value::NULL, |object| object.proto);
        self.get_property_with_receiver(p, base, atom, receiver)
            .map(Some)
    }

    fn load_eval_name(&mut self, p: &ResidualProgram, atom: Atom) -> Result<Value, JsError> {
        if self.direct_eval {
            let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.with_binding(p, object, key, atom)? {
                    return self.get_property(p, object, atom);
                }
            }
            if let Some(value) = self.dynamic_binding(self.frames.len().saturating_sub(1), atom) {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some(value) = self.direct_eval_lexical_value(p, atom) {
                return self.checked_binding_read(p, atom, value);
            }
            if let Some(value) = self.load_eval_frame_local(p, atom) {
                return self.checked_binding_read(p, atom, value);
            }
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
                match self.heap.get(frame.env) {
                    Some(Cell::Environment { slots, .. }) => slots[slot],
                    _ => Value::UNDEFINED,
                }
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
        let value = self.get_property(p, self.realm.globals, atom)?;
        if value.is_undefined() && self.own_property(self.realm.globals, atom).is_none() {
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        Ok(value)
    }

    fn direct_eval_lexical_binding(
        &self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Option<crate::bytecode::EvalBinding> {
        let frame = self.frames.last()?;
        let function = p.functions.get(frame.function as usize)?;
        function
            .binding_sites
            .iter()
            .find(|site| site.resume_pc as usize == frame.pc)?
            .bindings
            .iter()
            .find(|binding| binding.atom == atom)
            .copied()
    }

    fn prepare_direct_eval_var_bindings(&mut self, atoms: &[Atom]) {
        let Some(frame_index) = self.frames.len().checked_sub(1) else {
            return;
        };
        let frame = &self.frames[frame_index];
        let mut additions = atoms
            .iter()
            .copied()
            .filter(|atom| {
                !frame
                    .dynamic_bindings
                    .iter()
                    .any(|(candidate, _)| candidate == atom)
            })
            .map(|atom| {
                (
                    atom,
                    self.activation_binding_value(frame_index, atom)
                        .unwrap_or(Value::UNDEFINED),
                )
            })
            .collect::<Vec<_>>();
        self.frames[frame_index]
            .dynamic_bindings
            .append(&mut additions);
        let bindings = self.frames[frame_index].dynamic_bindings.clone();
        if self.frames[frame_index].captured {
            let env = self.frames[frame_index].env;
            if let Some(Cell::Environment {
                dynamic_bindings, ..
            }) = self.heap.get_mut(env)
            {
                *dynamic_bindings = bindings;
            }
        }
    }

    fn direct_eval_lexical_value(&self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let binding = self.direct_eval_lexical_binding(p, atom)?;
        let frame_index = self.frames.len().checked_sub(1)?;
        let frame = &self.frames[frame_index];
        let (environment, slot) = match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => (
                Some(self.capture_env(frame_index, depth)?),
                usize::from(slot),
            ),
            crate::bytecode::EvalBindingLocation::Local(slot) => {
                (frame.captured.then_some(frame.env), usize::from(slot))
            }
        };
        if let Some(environment) = environment {
            match self.heap.get(environment)? {
                Cell::Environment { slots, .. } => slots.get(slot).copied(),
                _ => None,
            }
        } else {
            frame.locals.get(slot).copied()
        }
    }

    fn store_direct_eval_lexical_value(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        let Some(binding) = self.direct_eval_lexical_binding(p, atom) else {
            return Ok(false);
        };
        if let Some(current) = self.direct_eval_lexical_value(p, atom) {
            self.checked_binding_read(p, atom, current)?;
        }
        match binding.kind {
            crate::bytecode::LexicalBindingKind::Immutable => {
                return Err(self.type_error(p, "assignment to immutable binding".into()));
            }
            crate::bytecode::LexicalBindingKind::FunctionName => {
                return if strict {
                    Err(self.type_error(p, "assignment to function name binding".into()))
                } else {
                    Ok(true)
                };
            }
            crate::bytecode::LexicalBindingKind::Mutable => {}
        }
        let Some(frame_index) = self.frames.len().checked_sub(1) else {
            return Ok(false);
        };
        let (environment, slot) = match binding.location {
            crate::bytecode::EvalBindingLocation::Capture { depth, slot } => {
                (self.capture_env(frame_index, depth), usize::from(slot))
            }
            crate::bytecode::EvalBindingLocation::Local(slot) => (
                self.frames[frame_index]
                    .captured
                    .then_some(self.frames[frame_index].env),
                usize::from(slot),
            ),
        };
        if let Some(environment) = environment {
            if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(environment)
                && let Some(local) = slots.get_mut(slot)
            {
                *local = value;
                return Ok(true);
            }
        } else if let Some(local) = self.frames[frame_index].locals.get_mut(slot) {
            *local = value;
            return Ok(true);
        }
        Ok(false)
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

    fn store_eval_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
        strict: bool,
        declaration: bool,
    ) -> Result<(), JsError> {
        if strict {
            let with_base = self
                .frames
                .last()
                .map_or(self.with_stack.len(), |frame| frame.with_base)
                .min(self.with_stack.len());
            let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
            let with_objects = self.with_stack[with_base..].to_vec();
            for object in with_objects.into_iter().rev() {
                if self.with_binding(p, object, key, atom)? {
                    return self.set_property_with_program_mode(p, object, atom, value, true);
                }
            }
            if self.current_frame_has_lexical_alias(p, atom) {
                return Err(self.type_error(p, "assignment to function name binding".into()));
            }
            if self.direct_eval
                && let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
                self.sync_dynamic_bindings();
                return Ok(());
            }
            if self.direct_eval && self.store_direct_eval_lexical_value(p, atom, value, strict)? {
                return Ok(());
            }
            if !self.parameter_eval && self.store_frame_local(p, atom, value) {
                return Ok(());
            }
            if let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
                self.sync_dynamic_bindings();
                return Ok(());
            }
            if self.own_property(self.realm.globals, atom).is_some() {
                self.store_frame_local(p, atom, value);
                return self.set_property_with_program_mode(
                    p,
                    self.realm.globals,
                    atom,
                    value,
                    strict,
                );
            }
            return Err(self.reference_error(p, format!("{} is not defined", self.atom_name(atom))));
        }
        if self.direct_eval {
            if !declaration && self.current_frame_has_lexical_alias(p, atom) {
                return Ok(());
            }
            if !declaration
                && let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
                self.sync_dynamic_bindings();
                return Ok(());
            }
            if self.direct_eval && self.store_direct_eval_lexical_value(p, atom, value, strict)? {
                return Ok(());
            }
            let global_frame = self.frames.last().is_some_and(|frame| frame.function == 0);
            if global_frame {
                self.store_frame_local(p, atom, value);
                if self.own_property(self.realm.globals, atom).is_some() {
                    return self.set_property_with_program_mode(
                        p,
                        self.realm.globals,
                        atom,
                        value,
                        strict,
                    );
                }
                return self.define_global_eval_binding(p, atom, value);
            }
            if !self.parameter_eval && self.store_frame_local(p, atom, value) {
            } else if !self.parameter_eval && self.own_property(self.realm.globals, atom).is_some()
            {
                self.store_frame_local(p, atom, value);
                self.store_eval_outer_local(p, atom, value);
                return self.set_property_with_program_mode(
                    p,
                    self.realm.globals,
                    atom,
                    value,
                    strict,
                );
            } else if let Some(frame) = self.frames.last_mut()
                && let Some((_, current)) = frame
                    .dynamic_bindings
                    .iter_mut()
                    .rev()
                    .find(|(candidate, _)| *candidate == atom)
            {
                *current = value;
                self.sync_dynamic_bindings();
            } else if declaration {
                let frame = self
                    .frames
                    .last_mut()
                    .expect("direct eval runs inside an activation");
                frame.dynamic_bindings.push((atom, value));
                self.sync_dynamic_bindings();
            } else {
                return self
                    .set_property_with_program_mode(p, self.realm.globals, atom, value, strict)
                    .map_err(|_| self.type_error(p, "cannot define global eval binding".into()));
            }
        } else {
            // Indirect eval targets the realm global environment, never the
            // caller's activation locals.
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        let with_base = self
            .frames
            .last()
            .map_or(self.with_stack.len(), |frame| frame.with_base)
            .min(self.with_stack.len());
        let with_objects = self.with_stack[with_base..].to_vec();
        for object in with_objects.into_iter().rev() {
            if self.with_binding(p, object, key, atom)? {
                return self.set_property_with_program_mode(p, object, atom, value, true);
            }
        }
        if self.direct_eval {
            Ok(())
        } else {
            self.set_property_with_program_mode(p, self.realm.globals, atom, value, strict)
                .map_err(|_| self.type_error(p, "cannot define global eval binding".into()))
        }
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

    fn store_eval_outer_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) {
        let current = self.frames.len().saturating_sub(1);
        if self.frames.get(current).is_some_and(|frame| {
            p.functions
                .get(frame.function as usize)
                .is_some_and(|function| function.local_atoms.contains(&atom))
        }) {
            return;
        }
        for index in (0..current).rev() {
            let (captured, env, slot) = {
                let frame = &self.frames[index];
                let Some(function) = p.functions.get(frame.function as usize) else {
                    continue;
                };
                let Some(slot) = function
                    .local_atoms
                    .iter()
                    .position(|candidate| *candidate == atom)
                else {
                    continue;
                };
                (frame.captured, frame.env, slot)
            };
            if captured {
                if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) {
                    if let Some(local) = slots.get_mut(slot) {
                        *local = value;
                        return;
                    }
                }
            } else if let Some(local) = self.frames[index].locals.get_mut(slot) {
                *local = value;
                return;
            }
        }
    }

    pub(super) fn load_eval_frame_local(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Option<Value> {
        let program = self.frames.last()?.program;
        let name = self.atom_name(atom).to_owned();
        let mut best: Option<(String, Value)> = None;
        for index in (0..self.frames.len()).rev() {
            let frame = &self.frames[index];
            if frame.program != program {
                continue;
            }
            if index != self.frames.len().saturating_sub(1)
                && (frame.function != super::ROOT_FUNCTION_ID
                    || frame.this != self.realm.globals
                    || self.own_property(self.realm.globals, atom).is_none())
            {
                continue;
            }
            let Some(function) = p.functions.get(frame.function as usize) else {
                continue;
            };
            for (slot, candidate) in function.local_atoms.iter().enumerate() {
                if self.root_global_var_atom(p, frame.function, slot).is_some() {
                    // Object-environment bindings must read the property,
                    // including its accessor, rather than a frame-local copy.
                    continue;
                }
                let candidate_name = self.atom_name(*candidate);
                if candidate_name != name
                    && !name.starts_with('\0')
                    && !(candidate_name.starts_with(&name)
                        && candidate_name[name.len()..]
                            .chars()
                            .all(|character| character == '_'))
                {
                    continue;
                }
                if best
                    .as_ref()
                    .is_some_and(|(best_name, _)| best_name.len() >= candidate_name.len())
                {
                    continue;
                }
                let value = if frame.captured {
                    self.heap.get(frame.env).and_then(|cell| match cell {
                        Cell::Environment { slots, .. } => slots.get(slot).copied(),
                        _ => None,
                    })
                } else {
                    frame.locals.get(slot).copied()
                }?;
                if value.is_deleted() {
                    continue;
                }
                best = Some((candidate_name.to_owned(), value));
            }
        }
        best.map(|(_, value)| value)
    }

    pub(super) fn load_eval_capture_atom(&self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame_index = self.frames.len().checked_sub(1)?;
        let frame = self.frames.get(frame_index)?;
        let function = p.functions.get(frame.function as usize)?;
        let parent = function.parent?;
        let slot = self.local_binding_slot(p, parent, atom)?;
        let env = self.capture_env(frame_index, 0)?;
        match self.heap.get(env)? {
            Cell::Environment { slots, .. } => slots.get(slot).copied(),
            _ => None,
        }
    }

    fn current_frame_has_lexical_alias(&self, p: &ResidualProgram, atom: Atom) -> bool {
        let name = self.atom_name(atom);
        let Some(frame) = self.frames.last() else {
            return false;
        };
        let Some(function) = p.functions.get(frame.function as usize) else {
            return false;
        };
        function.local_atoms.iter().any(|candidate| {
            let candidate_name = self.atom_name(*candidate);
            candidate_name.strip_prefix(name).is_some_and(|suffix| {
                suffix.starts_with("\0rqj:self-binding:")
                    || (!suffix.is_empty() && suffix.chars().all(|character| character == '_'))
            })
        })
    }

    pub(super) fn sync_dynamic_bindings(&mut self) {
        let Some(frame) = self.frames.last() else {
            return;
        };
        if !frame.captured {
            return;
        }
        let env = frame.env;
        let bindings = frame.dynamic_bindings.clone();
        if let Some(Cell::Environment {
            dynamic_bindings, ..
        }) = self.heap.get_mut(env)
        {
            *dynamic_bindings = bindings;
        }
    }

    fn store_frame_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) -> bool {
        for index in (0..self.frames.len()).rev() {
            let (captured, env, slot) = {
                let frame = &self.frames[index];
                if index != self.frames.len().saturating_sub(1)
                    && (frame.function != 0
                        || self.own_property(self.realm.globals, atom).is_none())
                {
                    continue;
                }
                let Some(function) = p.functions.get(frame.function as usize) else {
                    continue;
                };
                let Some(slot) = function
                    .local_atoms
                    .iter()
                    .position(|candidate| *candidate == atom)
                else {
                    continue;
                };
                if self.root_global_var_atom(p, frame.function, slot).is_some() {
                    return false;
                }
                (frame.captured, frame.env, slot)
            };
            if captured {
                if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env)
                    && let Some(local) = slots.get_mut(slot)
                {
                    *local = value;
                    return true;
                }
            } else if let Some(local) = self.frames[index].locals.get_mut(slot) {
                *local = value;
                return true;
            }
        }
        false
    }
}

fn private_identity_label(identity: &str) -> Option<String> {
    let identity = identity.strip_prefix("\0rqj:private:")?;
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
        eval_method_context_source(&statements, private_names),
    )
}

fn eval_method_context_source(source: &str, private_names: &[(String, String)]) -> String {
    let declarations = private_names
        .iter()
        .map(|(label, _)| format!("#{label};"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("(class {{\n{declarations}\n__eval() {{\n{source}\n}}\n}})")
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
                return flags.iter().all(|unit| {
                    char::from_u32(u32::from(*unit)).is_some_and(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
                    })
                }).then_some((&source[1..index], flags));
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

fn is_use_strict(statement: &str) -> bool {
    matches!(statement.trim(), "'use strict'" | "\"use strict\"")
}

fn strip_eval_comments(source: &str) -> Cow<'_, str> {
    let mut output = String::with_capacity(source.len());
    let mut input = source.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    let mut changed = false;
    while let Some(character) = input.next() {
        if let Some(delimiter) = quote {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            output.push(character);
            continue;
        }
        if character == '/' && input.peek() == Some(&'/') {
            input.next();
            changed = true;
            for character in input.by_ref() {
                if matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                    output.push(character);
                    break;
                }
            }
            continue;
        }
        if character == '/' && input.peek() == Some(&'*') {
            input.next();
            changed = true;
            let mut previous = '\0';
            let mut closed = false;
            for character in input.by_ref() {
                if matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                    output.push(character);
                }
                if previous == '*' && character == '/' {
                    closed = true;
                    break;
                }
                previous = character;
            }
            if !closed {
                return Cow::Borrowed(source);
            }
            output.push(' ');
            continue;
        }
        output.push(character);
    }
    if changed {
        Cow::Owned(output)
    } else {
        Cow::Borrowed(source)
    }
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

fn simple_eval_call(expression: &str) -> Option<(&str, &str)> {
    let open = expression.find('(')?;
    let name = expression[..open].trim();
    if !is_eval_identifier(name) {
        return None;
    }

    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in expression[open..].char_indices() {
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
            '\'' | '"' => quote = Some(character),
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let close = open + offset;
                    return expression[close + 1..]
                        .trim()
                        .is_empty()
                        .then_some((name, &expression[open + 1..close]));
                }
            }
            _ => {}
        }
    }
    None
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

fn contains_eval_identifier(source: &str, target: &str) -> bool {
    let bytes = source.as_bytes();
    let target = target.as_bytes();
    let mut index = 0;
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            if byte == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            index = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |offset| index + offset + 1);
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index = bytes[index + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(bytes.len(), |offset| index + offset + 4);
            continue;
        }
        if bytes[index..].starts_with(target) {
            let end = index + target.len();
            let identifier =
                |byte: u8| byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric();
            let starts_identifier = index == 0 || !identifier(bytes[index - 1]);
            let ends_identifier = end == bytes.len() || !identifier(bytes[end]);
            let mut previous = index;
            while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                previous -= 1;
            }
            if starts_identifier
                && ends_identifier
                && (previous == 0 || !matches!(bytes[previous - 1], b'.' | b'?'))
            {
                return true;
            }
            index = end;
            continue;
        }
        index += 1;
    }
    false
}

fn is_strict_reserved(name: &str) -> bool {
    matches!(
        name,
        "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
    )
}

fn split_assignment(statement: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    for (index, character) in statement.char_indices() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(current), character) if current == character => quote = None,
            (None, '=') => {
                let previous = statement[..index].chars().next_back();
                let next = statement[index + character.len_utf8()..].chars().next();
                if matches!(previous, Some('=' | '!' | '<' | '>'))
                    || matches!(next, Some('=' | '>'))
                {
                    continue;
                }
                let name = statement[..index].trim();
                if is_eval_identifier(name) {
                    return Some((name, statement[index + 1..].trim()));
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

fn is_eval_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|character| {
        character == '_' || character == '$' || character.is_ascii_alphabetic()
    }) && characters
        .all(|character| character == '_' || character == '$' || character.is_ascii_alphanumeric())
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

fn eval_super_property_key(source: &str) -> Option<String> {
    let property = source.strip_prefix("super.").and_then(|property| {
        property
            .chars()
            .all(|character| {
                character == '_' || character == '$' || character.is_ascii_alphanumeric()
            })
            .then(|| property.to_owned())
    });
    property.or_else(|| {
        let key = source.strip_prefix("super[")?.strip_suffix(']')?.trim();
        let quote = key.chars().next()?;
        (matches!(quote, '\'' | '"') && key.chars().last() == Some(quote))
            .then(|| key[1..key.len() - 1].to_owned())
    })
}
