use std::borrow::Cow;

use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn eval_script_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let previous_global = self.realm.globals;
        if let Some(global) = self.active_native_env()
            && self.object_data(global).is_some()
        {
            self.realm.globals = global;
        }
        let result = self.eval_script_native_in_realm(p, args);
        self.realm.globals = previous_global;
        result
    }

    fn eval_script_native_in_realm(
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
        let residual = match crate::Engine::specialize_unspecialized_with_atom_prefix(
            &source,
            &source_name,
            &atom_prefix,
        ) {
            Ok(residual) => residual,
            Err(diagnostics) => {
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
        let previous_global = self.realm.globals;
        if let Some(global) = self.active_native_env()
            && self.object_data(global).is_some()
        {
            self.realm.globals = global;
        }
        let result = self.eval_native_in_realm(p, args);
        self.realm.globals = previous_global;
        result
    }

    fn eval_native_in_realm(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::String(source_text)) = self.heap.get(source).cloned()
            && let Some(pattern_units) = eval_unflagged_regexp_literal(source_text.units())
        {
            let pattern = self
                .heap
                .alloc(Cell::String(JsString::from_units(pattern_units)));
            return self.construct_regexp_native(p, &[pattern, Value::UNDEFINED]);
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
        if inherited_strict && (trimmed.contains("arguments =") || trimmed.contains("arguments=")) {
            return self.syntax_error_result(p, "'arguments' is not allowed in strict mode");
        }
        if self.direct_eval && text.contains('#') {
            let private_names = self
                .direct_eval_private_names(p)
                .map_or_else(Vec::new, |(_, names)| names);
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
        let global_direct_eval = self.direct_eval
            && self
                .frames
                .last()
                .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID);
        match result {
            Err(error) if global_direct_eval && error.is_eval_parser_diagnostic() => {
                self.eval_global_script(p, &text)
            }
            result => result,
        }
    }

    fn eval_global_script(&mut self, p: &ResidualProgram, source: &str) -> Result<Value, JsError> {
        let source_name = format!("<Eval:{}>", self.programs.len());
        let strict = self
            .frames
            .last()
            .and_then(|frame| p.functions.get(frame.function as usize))
            .is_some_and(|function| function.strict);
        if let Some(expression) = crate::Engine::eval_single_expression(source) {
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
        )
        .map_err(|diagnostics| {
            let message = diagnostics
                .first()
                .map_or("invalid eval source".to_owned(), ToString::to_string);
            self.syntax_error_result(p, &message)
                .expect_err("dynamic eval syntax errors must throw")
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
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "dynamic program is unavailable".into()))?;
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
        let result = (|| {
            for atom in residual.functions[super::ROOT_FUNCTION_ID as usize]
                .global_var_atoms
                .iter()
                .copied()
            {
                if self.realm.global_lexical_declarations.contains(&atom)
                    || self.direct_eval_lexical_binding(p, atom).is_some()
                {
                    return self.syntax_error_result(
                        p,
                        "eval var declaration conflicts with lexical binding",
                    );
                }
                let globals = self.realm.globals;
                let name = self.atom_name(atom).to_owned();
                self.check_global_eval_declaration(p, globals, &name, false)?;
                if self.own_property(globals, atom).is_none() {
                    self.set_property(globals, atom, Value::UNDEFINED)?;
                    self.set_property_attributes(
                        globals,
                        PropertyKey::string(atom),
                        PropertyAttributes {
                            writable: true,
                            enumerable: true,
                            configurable: false,
                            accessor: false,
                            getter: None,
                            setter: None,
                        },
                    );
                }
            }
            let root_scope = self
                .frames
                .last()
                .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID);
            let parent = if direct_eval || root_scope {
                self.frames
                    .len()
                    .checked_sub(1)
                    .map_or(Value::NULL, |frame| self.promote_frame_environment(frame))
            } else {
                Value::NULL
            };
            let parent = if root_scope {
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
            };
            let root = self.closure(&residual, super::ROOT_FUNCTION_ID, parent)?;
            self.call_value(&residual, root, self.realm.globals, &[])
        })();
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
            crate::Engine::eval_var_names(source)
                .map(|names| names.declarations)
                .unwrap_or_default()
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
                    || self.direct_eval_lexical_binding(p, atom).is_some());
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
        if source.contains("new.target") {
            let invalid_context = !self.direct_eval
                || self.frames.last().is_none_or(|frame| {
                    if frame.function == 0 {
                        return true;
                    }
                    p.functions
                        .get(frame.function as usize)
                        .and_then(|function| function.name)
                        .is_some_and(|name| p.atoms[name as usize].as_bytes() == b"\0rqj:arrow")
                });
            if invalid_context {
                return self.syntax_error_result(p, "new.target is not valid in this eval context");
            }
        }
        if source.contains("super(") {
            return self.syntax_error_result(p, "super call is not valid in eval code");
        }
        if (source.contains("super.") || source.contains("super[")) && !self.direct_eval {
            return self.syntax_error_result(p, "super property is not valid in eval code");
        }
        if source.contains("\n++")
            || source.contains("for(;false;)")
            || source.trim_start().starts_with("return")
            || source.trim_start().starts_with("break")
            || source.trim_start().starts_with("continue")
        {
            return self.syntax_error_result(p, "invalid statement in eval code");
        }
        if crate::Engine::eval_requires_compiled_completion(source) {
            return self.eval_global_script(p, source);
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
        if source_strict && crate::Engine::eval_strict_eval_early_error(source) {
            return self.syntax_error_result(p, "assignment to eval is not allowed in strict mode");
        }
        if source_strict
            && let Some(error) = crate::Engine::eval_strict_binding_early_error(source, true)
        {
            let message = error.strip_prefix("SyntaxError: ").unwrap_or(&error);
            return self.syntax_error_result(p, message);
        }
        if source.contains("function")
            && !source.contains("super")
            && let Some(error) = crate::Engine::eval_parameter_early_error(source, source_strict)
        {
            let message = error.strip_prefix("SyntaxError: ").unwrap_or(&error);
            return self.syntax_error_result(p, message);
        }
        let has_eval_declarations =
            contains_eval_identifier(source, "var") || contains_eval_identifier(source, "function");
        if has_eval_declarations
            && !source_strict
            && self
                .frames
                .last()
                .is_some_and(|frame| frame.function == super::ROOT_FUNCTION_ID)
        {
            let global_lexicals = p
                .functions
                .first()
                .map(|root| root.global_lexical_atoms.clone())
                .unwrap_or_default();
            let eval_var_names = crate::Engine::eval_var_names(source)
                .map(|names| names.declarations)
                .unwrap_or_default();
            for name in eval_var_names {
                let atom = self.intern_atom(&name);
                let declared_lexically = self.realm.global_lexical_declarations.contains(&atom)
                    || global_lexicals.contains(&atom);
                if declared_lexically {
                    return self.syntax_error_result(
                        p,
                        "var declaration conflicts with global lexical binding",
                    );
                }
            }
        }
        if self.direct_eval && !source_strict {
            let globals = self.realm.globals;
            for statement in &statements {
                let Some(declarations) = statement.trim().strip_prefix("var ") else {
                    continue;
                };
                for declaration in split_commas(declarations) {
                    let name = declaration
                        .split_once('=')
                        .map_or(declaration.trim(), |(name, _)| name.trim());
                    let atom = self.intern_atom(name);
                    let parameter_conflict = self.parameter_eval
                        && self
                            .frames
                            .last()
                            .and_then(|frame| p.functions.get(frame.function as usize))
                            .is_some_and(|function| function.parameter_atoms.contains(&atom));
                    if parameter_conflict {
                        return self.syntax_error_result(
                            p,
                            "var declaration conflicts with parameter binding",
                        );
                    }
                    if !self.parameter_eval
                        && self.current_frame_has_lexical_conflict(p, atom)
                        && self.own_property(globals, atom).is_none()
                    {
                        return self.syntax_error_result(
                            p,
                            "var declaration conflicts with global lexical binding",
                        );
                    }
                }
            }
        }
        if !self.direct_eval && !source_strict {
            let globals = self.realm.globals;
            if statements.iter().any(|statement| {
                statement
                    .trim()
                    .strip_prefix("var ")
                    .is_some_and(|declarations| {
                        split_commas(declarations).into_iter().any(|declaration| {
                            let name = declaration
                                .split_once('=')
                                .map_or(declaration.trim(), |(name, _)| name.trim());
                            let atom = self.intern_atom(name);
                            self.current_frame_declares_global_lexical(p, atom)
                                && self.own_property(globals, atom).is_none()
                        })
                    })
            }) {
                return self.syntax_error_result(
                    p,
                    "var declaration conflicts with global lexical binding",
                );
            }
        }
        if !source_strict
            && (!self.direct_eval || self.frames.last().is_some_and(|frame| frame.function == 0))
        {
            let globals = self.realm.globals;
            for statement in &statements {
                let statement = statement.trim();
                if let Some(name) = function_declaration_name(statement) {
                    self.check_global_eval_declaration(p, globals, name, true)?;
                }
                if let Some(rest) = statement.strip_prefix("var ") {
                    for declaration in split_commas(rest) {
                        let name = declaration
                            .split_once('=')
                            .map_or(declaration.trim(), |(name, _)| name.trim());
                        self.check_global_eval_declaration(p, globals, name, false)?;
                        let lexical_atom = self.intern_atom(name);
                        let lexical_conflict = self.direct_eval
                            && self.current_frame_has_lexical_conflict(p, lexical_atom)
                            && self.own_property(globals, lexical_atom).is_none();
                        if lexical_conflict && !source_strict {
                            return self.syntax_error_result(
                                p,
                                "var declaration conflicts with global lexical binding",
                            );
                        }
                    }
                }
            }
        }
        let generator_eval_arguments = self.direct_eval
            && self
                .frames
                .last()
                .and_then(|frame| p.functions.get(frame.function as usize))
                .is_some_and(|function| function.is_generator)
            && statements.iter().any(|statement| {
                statement
                    .trim()
                    .strip_prefix("var ")
                    .is_some_and(|declarations| {
                        split_commas(declarations).into_iter().any(|declaration| {
                            declaration
                                .split_once('=')
                                .map_or(declaration.trim(), |(name, _)| name.trim())
                                == "arguments"
                        })
                    })
            });
        if generator_eval_arguments {
            return self
                .syntax_error_result(p, "arguments binding is not allowed in generator eval");
        }
        let strict = source_strict;
        if strict && self.direct_eval {
            let atoms = crate::Engine::eval_var_names(source)
                .map(|names| names.bindings)
                .unwrap_or_default()
                .into_iter()
                .map(|name| self.intern_atom(&name))
                .collect::<Vec<_>>();
            if let Some(frame) = self.frames.last_mut() {
                frame
                    .dynamic_bindings
                    .extend(atoms.into_iter().map(|atom| (atom, Value::UNDEFINED)));
            }
            self.sync_dynamic_bindings();
        }
        for statement in &statements {
            if function_declaration_name(statement.trim()).is_some() {
                self.install_eval_function(p, statement.trim(), strict)?;
            }
        }
        if !strict {
            for statement in &statements {
                let statement = statement.trim();
                let Some(declarations) = statement.strip_prefix("var ") else {
                    continue;
                };
                for declaration in split_commas(declarations) {
                    let name = declaration
                        .split_once('=')
                        .map_or(declaration.trim(), |(name, _)| name.trim());
                    let atom = self.intern_atom(name);
                    if self.load_eval_name(p, atom).is_err() {
                        if let Err(error) =
                            self.store_eval_name(p, atom, Value::UNDEFINED, false, true)
                        {
                            return Err(if error.thrown_value().is_some() {
                                error
                            } else {
                                self.type_error(p, error.into_message())
                            });
                        }
                    }
                }
            }
        }
        let mut result = crate::Engine::eval_directives(source)
            .and_then(|directives| directives.last().cloned())
            .map(|directive| self.heap.alloc(Cell::String(directive.into())))
            .unwrap_or(Value::UNDEFINED);
        for statement in statements {
            let statement = statement.trim();
            if statement.is_empty() || is_use_strict(statement) {
                continue;
            }
            if statement.starts_with("class ") {
                continue;
            }
            if is_empty_eval_statement(statement) {
                continue;
            }
            if crate::Engine::eval_is_function_declaration(statement) {
                continue;
            }
            if let Some(declarations) = statement
                .strip_prefix("var ")
                .or_else(|| statement.strip_prefix("let "))
                .or_else(|| statement.strip_prefix("const "))
            {
                let lexical = statement.starts_with("let ") || statement.starts_with("const ");
                for declaration in split_commas(declarations) {
                    let (name, expression) = declaration
                        .split_once('=')
                        .map_or((declaration.trim(), "undefined"), |(name, expression)| {
                            (name.trim(), expression.trim())
                        });
                    if strict && is_strict_reserved(name) {
                        return self.syntax_error_result(p, "reserved binding in strict eval");
                    }
                    let atom = self.intern_atom(name);
                    if self.direct_eval && name == "arguments" {
                        let parameter_binding = self
                            .frames
                            .last()
                            .and_then(|frame| p.functions.get(frame.function as usize))
                            .and_then(|function| {
                                function
                                    .local_atoms
                                    .iter()
                                    .position(|candidate| *candidate == atom)
                                    .map(|slot| slot < usize::from(function.params))
                            })
                            .unwrap_or(false);
                        if parameter_binding {
                            return self.syntax_error_result(
                                p,
                                "arguments binding conflicts with parameter",
                            );
                        }
                    }
                    if lexical && let Some(frame) = self.frames.last_mut() {
                        frame.dynamic_bindings.push((atom, Value::DELETED));
                    }
                    let value = self.eval_simple_expression(p, expression, strict)?;
                    if lexical {
                        if let Some(frame) = self.frames.last_mut()
                            && let Some((_, binding)) = frame
                                .dynamic_bindings
                                .iter_mut()
                                .rev()
                                .find(|(candidate, _)| *candidate == atom)
                        {
                            *binding = value;
                        }
                    } else {
                        if strict {
                            if self.direct_eval
                                && let Some((_, binding)) =
                                    self.frames.last_mut().and_then(|frame| {
                                        frame
                                            .dynamic_bindings
                                            .iter_mut()
                                            .rev()
                                            .find(|(candidate, _)| *candidate == atom)
                                    })
                            {
                                *binding = value;
                                self.sync_dynamic_bindings();
                            }
                            continue;
                        }
                        if self.direct_eval && !self.parameter_eval {
                            self.store_eval_local(p, atom, value);
                            self.store_eval_outer_local(p, atom, value);
                        }
                        if let Err(error) = self.store_eval_name(p, atom, value, strict, true) {
                            return Err(if error.thrown_value().is_some() {
                                error
                            } else {
                                self.type_error(p, error.into_message())
                            });
                        }
                    }
                }
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
            if let Some(name) = statement.strip_prefix("delete ") {
                let atom = self.intern_atom(name.trim());
                let eval_binding = self.frames.last().and_then(|frame| {
                    frame
                        .dynamic_bindings
                        .iter()
                        .rposition(|(candidate, _)| *candidate == atom)
                });
                if let Some(index) = eval_binding {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.dynamic_bindings.remove(index);
                    }
                    self.sync_dynamic_bindings();
                    result = Value::TRUE;
                } else {
                    result = self.delete_name(p, atom)?;
                }
                continue;
            }
            if let Some(expression) = statement.strip_prefix("throw ") {
                let value = self.eval_simple_expression(p, expression, strict)?;
                return Err(JsError::thrown(value, "eval throw".into()));
            }
            if let Some((block, expression)) = crate::Engine::eval_block_completion(statement) {
                let _ = self.eval_source_simple(p, block, strict)?;
                result = self.eval_simple_expression(p, expression, strict)?;
                continue;
            }
            if let Some(block) = crate::Engine::eval_block_statement(statement) {
                if !block.trim().is_empty() {
                    result = self.eval_source_simple(p, block, strict)?;
                }
                continue;
            }
            if let Some(expression) = crate::Engine::eval_labeled_expression(statement) {
                result = self.eval_simple_expression(p, expression, strict)?;
                continue;
            }
            if let Some((label, expression)) = statement.split_once(':')
                && is_eval_identifier(label.trim())
            {
                result = self.eval_simple_expression(p, expression, strict)?;
                continue;
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
            if let Some(pattern) = regexp_literal_pattern(&units[start..end]) {
                let pattern = self.heap.alloc(Cell::String(JsString::from_units(pattern)));
                let flags = self.heap.alloc(Cell::String(literal.flags.into()));
                return self.construct_regexp_native(p, &[pattern, flags]);
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
        if expression.starts_with("function") {
            let Some(body_start) = expression.find('{') else {
                return Ok(Value::UNDEFINED);
            };
            let Some(body_end) = expression.rfind('}') else {
                return Ok(Value::UNDEFINED);
            };
            let body = self.heap.alloc(Cell::String(
                expression[body_start + 1..body_end].trim().into(),
            ));
            return Ok(self.native_with_env(Native::DynamicFunction, body));
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
        if let Some(name) = expression.strip_prefix("++") {
            let atom = self.intern_atom(name.trim());
            let current = self.load_name(p, atom, 0)?;
            let value = Value::number(current.as_number().unwrap_or(0.0) + 1.0);
            self.store_eval_name(p, atom, value, strict, false)?;
            if !self.direct_eval && !strict {
                self.store_frame_local(p, atom, value);
                self.store_eval_outer_local(p, atom, value);
            }
            return Ok(value);
        }
        if let Some((name, rhs)) = expression.split_once("+=") {
            let atom = self.intern_atom(name.trim());
            let current = self.load_name(p, atom, 0)?;
            let increment = self.eval_simple_expression(p, rhs, strict)?;
            let value = Value::number(
                current.as_number().unwrap_or(0.0) + increment.as_number().unwrap_or(0.0),
            );
            self.store_eval_name(p, atom, value, strict, false)?;
            if !self.direct_eval && !strict {
                self.store_frame_local(p, atom, value);
                self.store_eval_outer_local(p, atom, value);
            }
            return Ok(value);
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
        let message = diagnostics
            .first()
            .map_or("invalid eval expression".to_owned(), ToString::to_string);
        self.syntax_error_result(p, &message)
            .expect_err("dynamic eval syntax errors must throw")
            .mark_eval_parser_diagnostic()
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
            .flat_map(|object| &object.private_names)
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

    fn install_eval_function(
        &mut self,
        p: &ResidualProgram,
        statement: &str,
        strict: bool,
    ) -> Result<(), JsError> {
        let rest = statement
            .strip_prefix("function ")
            .or_else(|| statement.strip_prefix("function* "))
            .unwrap_or_default();
        let Some(open) = rest.find('(') else {
            return Ok(());
        };
        let name = rest[..open].trim();
        let Some(body_start) = statement.find('{') else {
            return Ok(());
        };
        let Some(body_end) = statement.rfind('}') else {
            return Ok(());
        };
        if name.is_empty() || body_end <= body_start {
            return Ok(());
        }
        if strict {
            return Ok(());
        }
        let body = self.heap.alloc(Cell::String(
            statement[body_start + 1..body_end].trim().into(),
        ));
        let function = self.native_with_env(Native::DynamicFunction, body);
        let atom = self.intern_atom(name);
        if !self.direct_eval || self.frames.last().is_some_and(|frame| frame.function == 0) {
            if let Some(attributes) = self.property_attributes(
                self.realm.globals,
                crate::vm::property_key::PropertyKey::string(atom),
            ) {
                if attributes.configurable {
                    self.set_property_attributes(
                        self.realm.globals,
                        crate::vm::property_key::PropertyKey::string(atom),
                        PropertyAttributes {
                            writable: true,
                            enumerable: true,
                            configurable: true,
                            accessor: false,
                            getter: None,
                            setter: None,
                        },
                    );
                } else if !attributes.writable || attributes.accessor {
                    return Err(self.type_error(p, "cannot redefine global eval function".into()));
                }
                self.set_field_cached(p, self.realm.globals, atom, function, 0, false)?;
                return Ok(());
            }
            return self.define_global_eval_binding(p, atom, function);
        }
        self.store_eval_name(p, atom, function, false, true)
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
            return self.load_name(p, atom, 0);
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
        let value = self.get_field_cached(p, self.realm.globals, atom, 0)?;
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
            .eval_sites
            .iter()
            .find(|site| site.resume_pc as usize == frame.pc)?
            .lexical_bindings
            .iter()
            .find(|binding| binding.atom == atom)
            .copied()
    }

    fn direct_eval_lexical_value(&self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame = self.frames.last()?;
        let slot = usize::from(self.direct_eval_lexical_binding(p, atom)?.slot);
        if frame.captured {
            match self.heap.get(frame.env)? {
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
    ) -> Result<bool, JsError> {
        let Some(binding) = self.direct_eval_lexical_binding(p, atom) else {
            return Ok(false);
        };
        if binding.immutable {
            return Err(self.type_error(p, "assignment to immutable binding".into()));
        }
        let Some(frame) = self.frames.last_mut() else {
            return Ok(false);
        };
        let slot = usize::from(binding.slot);
        if frame.captured {
            if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(frame.env)
                && let Some(local) = slots.get_mut(slot)
            {
                *local = value;
                return Ok(true);
            }
        } else if let Some(local) = frame.locals.get_mut(slot) {
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
            if self.direct_eval && self.store_direct_eval_lexical_value(p, atom, value)? {
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
                return self.set_field_cached(p, self.realm.globals, atom, value, 0, false);
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
            if self.direct_eval && self.store_direct_eval_lexical_value(p, atom, value)? {
                return Ok(());
            }
            let global_frame = self.frames.last().is_some_and(|frame| frame.function == 0);
            if global_frame {
                self.store_frame_local(p, atom, value);
                if self.own_property(self.realm.globals, atom).is_some() {
                    return self.set_field_cached(p, self.realm.globals, atom, value, 0, false);
                }
                return self.define_global_eval_binding(p, atom, value);
            }
            if !self.parameter_eval && self.store_frame_local(p, atom, value) {
            } else if !self.parameter_eval && self.own_property(self.realm.globals, atom).is_some()
            {
                self.store_frame_local(p, atom, value);
                self.store_eval_outer_local(p, atom, value);
                return self.set_field_cached(p, self.realm.globals, atom, value, 0, false);
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
                    .set_field_cached(p, self.realm.globals, atom, value, 0, false)
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
            self.set_field_cached(p, self.realm.globals, atom, value, 0, false)
                .map_err(|_| self.type_error(p, "cannot define global eval binding".into()))
        }
    }

    fn define_global_eval_binding(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        let descriptor = self.object();
        for (name, field) in [
            ("value", value),
            ("writable", Value::TRUE),
            ("enumerable", Value::TRUE),
            ("configurable", Value::TRUE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field)?;
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        self.object_define_property(p, &[self.realm.globals, key, descriptor])
            .map(|_| ())
            .map_err(|_| self.type_error(p, "cannot define global eval binding".into()))
    }

    fn store_eval_local(&mut self, p: &ResidualProgram, atom: Atom, value: Value) {
        let Some(frame) = self.frames.last() else {
            return;
        };
        let Some(function) = p.functions.get(frame.function as usize) else {
            return;
        };
        let Some(slot) = function
            .local_atoms
            .iter()
            .position(|candidate| *candidate == atom)
        else {
            return;
        };
        if frame.captured {
            if let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(frame.env)
                && let Some(local) = slots.get_mut(slot)
            {
                *local = value;
            }
        } else if let Some(local) = self
            .frames
            .last_mut()
            .and_then(|frame| frame.locals.get_mut(slot))
        {
            *local = value;
        }
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
        let name = self.atom_name(atom).to_owned();
        let mut best: Option<(String, Value)> = None;
        for index in (0..self.frames.len()).rev() {
            let frame = &self.frames[index];
            if index != self.frames.len().saturating_sub(1)
                && (frame.function != 0 || self.own_property(self.realm.globals, atom).is_none())
            {
                continue;
            }
            let Some(function) = p.functions.get(frame.function as usize) else {
                continue;
            };
            for (slot, candidate) in function.local_atoms.iter().enumerate() {
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

    fn current_frame_has_lexical_conflict(&self, p: &ResidualProgram, atom: Atom) -> bool {
        if self.current_frame_has_lexical_alias(p, atom) {
            return true;
        }
        let Some(frame) = self.frames.last() else {
            return false;
        };
        p.functions
            .get(frame.function as usize)
            .is_some_and(|function| function.lexical_atoms.contains(&atom))
    }

    fn current_frame_declares_global_lexical(&self, p: &ResidualProgram, atom: Atom) -> bool {
        let Some(frame) = self.frames.last() else {
            return false;
        };
        frame.function == 0
            && !self.current_frame_has_lexical_alias(p, atom)
            && p.functions
                .get(frame.function as usize)
                .is_some_and(|function| function.lexical_atoms.contains(&atom))
    }

    fn sync_dynamic_bindings(&mut self) {
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
    let declarations = private_names
        .iter()
        .map(|(label, _)| format!("#{label};"))
        .collect::<Vec<_>>()
        .join("\n");
    let statements = if expression {
        format!("return ({source});")
    } else {
        source.to_owned()
    };
    format!(
        "return (class {{\n{declarations}\n__eval() {{ {statements}\n}} }}).prototype.__eval.call(this);"
    )
}

fn regexp_literal_pattern(literal: &[u16]) -> Option<&[u16]> {
    if literal.first().copied() != Some(u16::from(b'/')) {
        return None;
    }
    let mut escaped = false;
    for (index, unit) in literal.iter().copied().enumerate().skip(1) {
        if unit == u16::from(b'/') && !escaped {
            return Some(&literal[1..index]);
        }
        escaped = unit == u16::from(b'\\') && !escaped;
        if unit != u16::from(b'\\') {
            escaped = false;
        }
    }
    None
}

fn eval_unflagged_regexp_literal(source: &[u16]) -> Option<&[u16]> {
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
                return (index + 1 == source.len()).then_some(&source[1..index]);
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

fn function_declaration_name(statement: &str) -> Option<&str> {
    let rest = statement
        .strip_prefix("function ")
        .or_else(|| statement.strip_prefix("function* "))?;
    rest.split_once('(')
        .map(|(name, _)| name.trim())
        .filter(|name| !name.is_empty())
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

fn split_commas(source: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (index, character) in source.char_indices() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(current), character) if current == character => quote = None,
            (None, ',') => {
                result.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(source[start..].trim());
    result
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
