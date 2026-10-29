use crate::bytecode::private_identity_label;
use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

use crate::compile::eval_context_source;

/// Whether source contains only ECMAScript whitespace and comments. This is a
/// useful fast path for eval's common comment-only inputs: parsing an empty
/// Script has no bindings, side effects, or completion value other than
/// `undefined`.
fn only_trivia(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ' => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() {
                    if matches!(bytes[index], b'\n' | b'\r') {
                        break;
                    }
                    if bytes[index..].starts_with(&[0xe2, 0x80])
                        && matches!(bytes.get(index + 2), Some(0xa8 | 0xa9))
                    {
                        break;
                    }
                    let Some(character) = source[index..].chars().next() else {
                        return false;
                    };
                    index += character.len_utf8();
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let Some(end) = bytes[index + 2..]
                    .windows(2)
                    .position(|window| window == b"*/")
                else {
                    return false;
                };
                index += end + 4;
            }
            _ => {
                let Some(character) = source[index..].chars().next() else {
                    return false;
                };
                // ECMAScript WhiteSpace includes Unicode Space_Separator
                // characters; Rust additionally treats NEL as whitespace.
                if character != '\u{0085}'
                    && (character == '\u{feff}' || character.is_whitespace())
                {
                    index += character.len_utf8();
                } else {
                    return false;
                }
            }
        }
    }
    true
}


/// Names a specialized script's parse goal in its compile-cache key, apart from a host
/// script's and an application entry's (`Runtime::compile_entry`).
const SPECIALIZED_SCRIPT_GOAL: &str = "specialized-script";

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
        if only_trivia(source) {
            return Ok(Value::UNDEFINED);
        }
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
                        .map_or("invalid script source".to_owned(), |diagnostic| diagnostic.syntax_message().to_owned());
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
        if let Some(source_text) = self.heap.string(source).cloned()
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
                .alloc_string(JsString::from_units(pattern_units));
            let flags = self
                .heap
                .alloc_string(JsString::from_units(&flag_units));
            return self.construct_regexp_native(p, &[pattern, flags], None);
        }
        let source_text = match self.heap.get(source) {
            _ if let Some(value) = self.heap.string(source)  => value.clone(),
            _ => return Ok(source),
        };
        let cache_source = (!self.direct_eval).then(|| source_text.clone());
        let text = source_text.host_string();
        let inherited_strict = self.direct_eval
            && (self.in_class_initializer(p)
                || self.frames.last()
                    .and_then(|frame| p.function_get(frame.function as usize))
                    .is_some_and(|function| function.strict));
        self.validate_eval_arguments_context(p, text)?;
        if only_trivia(text) {
            return Ok(Value::UNDEFINED);
        }
        let source_name = format!("<Eval:{}>", self.programs.len());
        self.eval_global_script_with_cache(p, text, inherited_strict, &source_name, cache_source, false)
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
        self.eval_global_script_with_cache(p, source, strict, source_name, None, false)
    }

    /// A host's script, which is sloppy global code no guest frame calls: its compilation goes
    /// through the host's compile cache (`Host::kept_compiled_program`).
    pub(super) fn eval_host_script(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        source_name: &str,
    ) -> Result<Value, JsError> {
        self.eval_global_script_with_cache(p, source, false, source_name, None, true)
    }

    fn eval_global_script_with_cache(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        strict: bool,
        source_name: &str,
        cache_source: Option<JsString>,
        host_script: bool,
    ) -> Result<Value, JsError> {
        if let Some(program) = cache_source.as_ref().and_then(|source| self.programs.cached_global_eval(source)) {
            return self.run_global_eval_program(p, program);
        }
        if strict {
            self.validate_strict_eval(p, source)?;
        }
        let context = self.direct_eval_context(p).unwrap_or_default();
        let private_names = if self.direct_eval {
            self.direct_eval_private_names(p)
                .map_or_else(Vec::new, |(_, names)| names)
        } else {
            Vec::new()
        };
        let caller_has_global_variables = self.frames.is_empty() || self.running_global_code();
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
        // A host script compiles from its text alone: it is no direct eval, so it has no
        // caller context, private names or Annex B restrictions.
        let host_script = host_script && !self.direct_eval;
        let mode = self.specialization_part();
        let kept = host_script
            .then(|| self.host.kept_compiled_program(source_name, &[mode, source]))
            .flatten()
            .and_then(|bytes| {
                let text = crate::bytecode::SourceText::Shared(std::sync::Arc::from(source));
                let specialized = self.specialized;
                self.decode_cached_program(bytes, text, 0, source_name, specialized).ok()
            });
        let compiled = kept.is_none();
        let residual = match kept {
            Some(residual) => Ok(residual),
            None => crate::Engine::specialize_eval_with_context(
            source,
            source_name,
            &*self,
            strict,
            context,
            &private_names,
            &annex_b_forbidden_names,
        ),
        }
        .map_err(|diagnostics| {
            if diagnostics
                .iter()
                .any(crate::compile::Diagnostic::is_stack_exhausted)
            {
                return self.stack_exhaustion_error();
            }
            let message = diagnostics
                .first()
                .map_or("invalid eval source".to_owned(), |diagnostic| diagnostic.syntax_message().to_owned());
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
        if host_script && compiled
            && let Some(program) = self.programs.get(program_id)
            && let Some(bytes) = self.encode_cached_program(&program)
        {
            self.host.keep_compiled_program(source_name, &[mode, source], &bytes);
        }
        let origin = self.capture_eval_origin(p)?;
        self.programs.set_origin(program_id, super::program_store::CompilationOrigin::Eval { caller: Some(origin) });
        // Eval code belongs to its caller's Script or Module (PerformEval).
        let script_or_module = self.active_script_or_module(p);
        self.programs.set_script_or_module(program_id, script_or_module);
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

    /// `evaluate_embedding_specialized_script_with_origin` for a program whose functions keep
    /// their compiled records instead of compiling again on first call: an application's
    /// entry, whose own functions mostly run.
    pub(crate) fn evaluate_embedding_entry_script_with_origin(
        &mut self,
        source: &str,
        name: &str,
        origin: Option<(u32, u32)>,
    ) -> Result<Value, JsError> {
        let prepared =
            self.prepare_embedding_specialized_script_retaining(source, name, origin, None, true)?;
        self.evaluate_embedding_prepared_script(&prepared)
    }

    /// Compiles a host wrapper `prefix + body + suffix` around a library module's text: the
    /// program's positions start at the wrapper origin and it retains the body itself (static
    /// embedded text, or text the host derived from it), not a wrapped copy.
    fn prepare_embedding_library_body(
        &mut self, prefix: &str, body: crate::bytecode::SourceText, suffix: &str, name: &str,
    ) -> Result<crate::PreparedScript, JsError> {
        let wrapped = [prefix, &body, suffix].concat();
        let origin = crate::api::wrapper_origin(prefix)
            .ok_or_else(|| JsError("host wrapper origin exceeds compiler positions".into()))?;
        let base = u32::try_from(prefix.len())
            .map_err(|_| JsError("host wrapper prefix exceeds compiler positions".into()))?;
        self.prepare_embedding_specialized_script_retaining(&wrapped, name, Some(origin), Some((body, base)), true)
    }

    /// `prepare_embedding_library_body` through the host's compile cache
    /// (`Host::kept_compiled_program`): a kept program that decodes for this VM is used as is;
    /// otherwise the body compiles and its compact form is offered to the host.
    pub(crate) fn prepare_embedding_library_body_kept(
        &mut self, prefix: &str, body: crate::bytecode::SourceText, suffix: &str, name: &str,
    ) -> Result<crate::PreparedScript, JsError> {
        let parts = [self.specialization_part(), prefix, &*body, suffix];
        if let Some(bytes) = self.host.kept_compiled_program(name, &parts)
            && let Ok(program) = self.decode_cached_program(bytes, body.clone(), prefix.len(), name, self.specialized)
        {
            return Ok(crate::PreparedScript { program: Rc::new(program) });
        }
        let prepared = self.prepare_embedding_library_body(prefix, body.clone(), suffix, name)?;
        if let Some(bytes) = self.encode_cached_program(&prepared.program) {
            self.host.keep_compiled_program(name, &parts, &bytes);
            drop(bytes);
            // The module then runs from its kept form, as every later run's does, like an
            // entry script (`prepare_embedding_specialized_script_retaining`).
            if let Some(bytes) = self.host.kept_compiled_program(name, &parts)
                && let Ok(program) = self.decode_cached_program(bytes, body, prefix.len(), name, self.specialized)
            {
                return Ok(crate::PreparedScript { program: Rc::new(program) });
            }
        }
        Ok(prepared)
    }

    /// `program`'s compile-cache form, when every atom it names is found again by its text:
    /// a VM atom whose UTF-16 has no lossless host spelling cannot be cached by name.
    /// Names this VM's specialization in a compile-cache key: a program compiled under the
    /// other does not decode here (`decode_cached_program`). Without it, processes that run
    /// a module under each specialization replaced each other's entry, and each compiled it
    /// again (typescript.js, 9 MB of source, about 3 s per process).
    pub(super) fn specialization_part(&self) -> &'static str {
        if self.specialized { "specialized" } else { "unspecialized" }
    }

    pub(super) fn encode_cached_program(&self, program: &crate::ResidualProgram) -> Option<Vec<u8>> {
        // The compilation is over either way (`CompilePhase::Encoding`); a host that keeps no
        // programs is offered none, so nothing is encoded (async-fs, cold: 43 encodings,
        // 2.4% of its instructions).
        crate::compile::compile_phase_boundary(crate::compile::CompilePhase::Encoding);
        if !self.host.keeps_compiled_programs() {
            return None;
        }
        let (bytes, atoms) = program.encode_compact();
        atoms
            .iter()
            .all(|&atom| {
                let name = &program.atoms[atom as usize];
                let hash = Self::atom_hash_str(name);
                self.find_atom(hash, |found| self.atom_units_equal_str(found, name)) == Some(atom)
            })
            .then_some(bytes)
    }

    /// A program `encode_cached_program` encoded, with this VM's atom ids and `text` as its
    /// source, which must be what it compiled from `name` after a `base`-byte prefix, with
    /// `specialized` as its specialization.
    pub(super) fn decode_cached_program(
        &mut self, bytes: crate::KeptBytes, text: crate::bytecode::SourceText, base: usize, name: &str,
        specialized: bool,
    ) -> Result<crate::ResidualProgram, String> {
        let read = bytes.clone();
        let mut program = crate::ResidualProgram::decode_compact(
            bytes,
            &mut |name| self.intern_atom(name),
            text,
        )?;
        // The decode read the body to check and decode it; the program reads the bytes again
        // only for a function's record on its first call. A run that loads many modules
        // before its first release had kept every decoded body resident (undici npm scenario:
        // 4.2 MB of compile-cache pages at its 30 MB peak).
        H::kept_bytes_read((*read).as_ref());
        let source_matches = program.source.as_ref().is_some_and(|source| source.base as usize == base);
        if program.source_name != name || !source_matches || program.specialized != specialized {
            return Err("cached program does not match its body".into());
        }
        program.atoms =
            crate::bytecode::AtomTable::shared(self.atom_sequence.clone(), self.atom_sequence.len());
        // The bytes are this build's compiler output (the host keys them by build, and their
        // checksum held), which runs unvalidated when compiled in place; the decoder checked
        // their structure and every atom is now one of this VM's.
        self.host.cached_program_decoded();
        Ok(program)
    }

    /// The script `source` as the host's compile cache keeps it under `goal`, decoded for this
    /// VM with its origin, when the cache holds it.
    fn kept_script(
        &mut self, source: &str, name: &str, goal: &str, origin: Option<(u32, u32)>,
    ) -> Option<crate::PreparedScript> {
        let entry = self.host.kept_compiled_program(name, &[self.specialization_part(), goal, source])?;
        let (bytes, text) = crate::bytecode::split_kept_source(entry, source.len())
            .unwrap_or_else(|entry| (entry, crate::bytecode::SourceText::Shared(std::sync::Arc::from(source))));
        let specialized = self.specialized;
        let mut kept = self.decode_cached_program(bytes, text, 0, name, specialized).ok()?;
        if let (Some(source), Some((line_offset, column_offset))) = (&mut kept.source, origin) {
            source.line_offset = line_offset;
            source.column_offset = column_offset;
        }
        Some(crate::PreparedScript { program: Rc::new(kept) })
    }

    pub(crate) fn prepare_embedding_specialized_script(
        &mut self, source: &str, name: &str, origin: Option<(u32, u32)>,
    ) -> Result<crate::PreparedScript, JsError> {
        self.prepare_embedding_specialized_script_retaining(source, name, origin, None, false)
    }

    /// `retained` names static text the program keeps in place of its compiled source, and
    /// the compiled byte offset where that text starts.
    fn prepare_embedding_specialized_script_retaining(
        &mut self, source: &str, name: &str, origin: Option<(u32, u32)>,
        retained: Option<(crate::bytecode::SourceText, u32)>,
        keep_records: bool,
    ) -> Result<crate::PreparedScript, JsError> {
        let program = self.embedding_program()?;
        // A script compiled from its own text (a package's module, an application's entry) goes
        // through the host's compile cache, keyed by its name, its text and what else shapes
        // its program: its origin and whether its functions keep their records.
        let cache_key = retained.is_none().then(|| {
            let (line, column) = origin.unwrap_or_default();
            format!("{SPECIALIZED_SCRIPT_GOAL}:{line}:{column}:{keep_records}")
        });
        if let Some(goal) = &cache_key
            && let Some(kept) = self.kept_script(source, name, goal, origin)
        {
            return Ok(kept);
        }
        // A program keeping only static text cannot compile its functions again from it.
        let compiled = if keep_records {
            crate::Engine::specialize_kept_script_with_atom_prefix(source, name, &*self)
        } else {
            crate::Engine::specialize_script_with_atom_prefix(source, name, &*self)
        };
        let mut residual = compiled
            .map_err(|diagnostics| {
                if diagnostics
                    .iter()
                    .any(crate::compile::Diagnostic::is_stack_exhausted)
                {
                    return self.stack_exhaustion_error();
                }
                let message = diagnostics
                    .first()
                    .map_or("invalid host script".to_owned(), |diagnostic| diagnostic.syntax_message().to_owned());
                self.syntax_error_result(&program, &message)
                    .expect_err("specialized host-script syntax errors must throw")
            })?;
        if let (Some(source), Some((line_offset, column_offset))) = (&mut residual.source, origin) {
            source.line_offset = line_offset;
            source.column_offset = column_offset;
        }
        if let (Some(source), Some((text, base))) = (&mut residual.source, retained) {
            source.text = text;
            source.base = base;
        }
        let mut residual = self.intern_program_atoms(residual);
        // Kept before its records are released: encoding reads every record. Every distinct
        // text a user runs adds an entry; a store that fails (a full disk) keeps the cache
        // as it was, and costs only the compilation it would have saved.
        if let Some(goal) = &cache_key
            && let Some(mut bytes) = self.encode_cached_program(&residual)
        {
            crate::bytecode::append_kept_source(&mut bytes, source);
            let mode = self.specialization_part();
            self.host.keep_compiled_program(name, &[mode, goal, source], &bytes);
            drop(bytes);
            // The program then runs from its kept form, as every later run of the text does:
            // its records decode on first call from the host's mapped entry, which the host
            // can page out, instead of staying compiled in memory for the whole run (V8-v7
            // MemoryFirst max RSS of a cold entry, static x86_64, 5 pairs, 2026-10-24:
            // NavierStokes 4708 -> 4172 KiB, RayTrace 3332 -> 3016, Crypto 3480 -> 3292,
            // RegExp 6228 -> 6028; a whole cold typescript-eslint run's CPU unchanged).
            if let Some(kept) = self.kept_script(source, name, goal, origin) {
                return Ok(kept);
            }
        }
        // Its final source is settled, so a function compiles again from it on first call.
        #[cfg(feature = "profile-aggregate")]
        let retain_profile_records = std::env::var_os("QUENCH_PROFILE_RETAIN_BYTECODE").is_some();
        #[cfg(not(feature = "profile-aggregate"))]
        let retain_profile_records = false;
        if !retain_profile_records {
            residual.release_replayable_records();
        }
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

    /// Whether the running frame is the root of code whose variables are the
    /// realm's global bindings: a Script, an indirect eval or host script, or a
    /// sloppy direct eval called from such code. The root of a direct eval
    /// running in a function's or module's scope is not.
    fn running_global_code(&self) -> bool {
        self.frames.last().is_some_and(|frame| {
            frame.function == super::ROOT_FUNCTION_ID
                && self.programs.declares_global_variables(frame.program)
        })
    }

    fn run_global_eval_program(
        &mut self,
        p: &ResidualProgram,
        program_id: super::ProgramId,
    ) -> Result<Value, JsError> {
        let class_initializer = self.direct_eval && self.in_class_initializer(p);
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "dynamic program is unavailable".into()))?;
        let active_program = self.replace_active_program(program_id);
        let direct_eval = std::mem::replace(&mut self.direct_eval, false);
        if !direct_eval {
            self.programs.mark_global_scope(program_id);
        }
        let root_function = residual.function(super::ROOT_FUNCTION_ID as usize).clone();
        let root_scope = self.running_global_code();
        let caller_scope = direct_eval && !root_scope && !root_function.strict;
        if direct_eval && !root_function.strict {
            // A sloppy direct eval declares its variables where its caller does,
            // so direct evals nested in it do too.
            if root_scope {
                self.programs.mark_global_variables(program_id);
            } else {
                self.programs.mark_caller_scope(program_id);
            }
        }
        let previous_eval_var_program = std::mem::replace(
            &mut self.direct_eval_var_program,
            caller_scope.then_some(program_id),
        );
        let result = (|| {
            if !root_function.strict {
                let global_function_atoms = residual.function(super::ROOT_FUNCTION_ID as usize)
                    .global_function_atoms()
                    .to_vec();
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
                for atom in residual.function(super::ROOT_FUNCTION_ID as usize)
                    .global_var_atoms()
                    .iter()
                    .copied()
                {
                    if caller_scope
                        && self.parameter_eval
                        && self
                            .frames
                            .last()
                            .and_then(|frame| p.function_get(frame.function as usize))
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
                    for atom in root_function.global_var_atoms().iter().copied() {
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
                self.prepare_direct_eval_var_bindings(&root_function.global_var_atoms());
            }
            let parent = if direct_eval {
                if let Some(frame) = self.frames.len().checked_sub(1) {
                    self.capture_binding_environment(frame)?
                } else {
                    Value::NULL
                }
            } else {
                Value::NULL
            };
            let parent = if direct_eval && (root_scope || class_initializer) {
                let dynamic_bindings = if class_initializer {
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
                    scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
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
                        .function_get(self.frames[frame].function as usize)
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
            scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: scope.program(),
                root_eval_scope: scope.root_eval_scope(),
                binding_site_pc: Some(binding_site_pc),
                dynamic_bindings: crate::heap::EnvironmentBindings::Shared(owner),
                with_objects: scope.with_objects().into(),
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
            && self.in_class_initializer(p)
            && crate::Engine::class_eval_references_arguments(source)
        {
            return self
                .syntax_error_result(
                    p,
                    "'arguments' is not allowed in class field initializer or static initialization block",
                )
                .map(drop);
        }
        Ok(())
    }

    fn in_class_initializer(&mut self, p: &ResidualProgram) -> bool {
        self.frames
            .last()
            .and_then(|frame| p.function_get(frame.function as usize))
            .is_some_and(|function| function.class_initializer)
    }

    fn direct_eval_function_context(&mut self, p: &ResidualProgram) -> bool {
        if !self.direct_eval {
            return false;
        }
        if self.in_class_initializer(p) {
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
            CallTarget::User(_, function, environment) => (function, environment),
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

    fn direct_eval_private_names(&mut self, p: &ResidualProgram) -> Option<(Value, Vec<(String, String)>)> {
        let frame = self.frames.len().checked_sub(1)?;
        let mut atoms = Vec::new();
        let metadata = p.function_get(self.frames[frame].function as usize)?;
        if let Some(pc) = self.frames[frame].binding_site_pc
            && let Ok(index) = metadata.binding_sites.binary_search_by_key(&pc, |site| site.resume_pc)
        {
            atoms.extend(metadata.binding_sites[index].bindings.iter().filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName).map(|binding| binding.atom));
        }
        atoms.extend(metadata.name_bindings.resolved().into_iter().filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName).map(|binding| binding.atom));
        let mut environment = self.captured_parent_environment(frame);
        while let Some(Cell::Environment { parent, function, scope, .. }) = self.heap.get(environment) {
            if let Some(owner) = scope.program().and_then(|program| self.programs.get(ProgramId::from_raw(program)))
                && let Some(metadata) = owner.function_get(*function as usize)
            {
                atoms.extend(metadata.name_bindings.resolved().into_iter().filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName).map(|binding| binding.atom));
            }
            environment = *parent;
        }
        atoms.sort_unstable();atoms.dedup();
        let mut bindings = atoms.into_iter().filter_map(|atom| self.name_binding(frame, atom)).filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName).collect::<Vec<_>>();
        bindings.sort_by_key(|binding| match binding.location { crate::bytecode::EvalBindingLocation::Local(_) => 0, crate::bytecode::EvalBindingLocation::Capture { depth, .. } => u32::from(depth)+1 });
        let mut names = Vec::new();
        let mut first_home = None;
        for binding in bindings {
            let identity = self.atom_name(binding.atom).to_owned();
            let Some(label) = private_identity_label(&identity) else { continue; };
            if names.iter().any(|(name, _)| name == &label) { continue; }
            if let Ok(home) = self.load_name_binding(p, frame, binding) {
                first_home.get_or_insert(home);
                names.push((label, identity));
            }
        }
        if !names.is_empty() { return Some((first_home?, names)); }
        let home = self.eval_home_binding(p)?;
        let names = self.object_data(home).into_iter().flat_map(|object| object.private_names()).filter_map(|brand| {
            let identity = self.atom_name(brand.name).to_owned();
            private_identity_label(&identity).map(|label| (label, identity))
        }).fold(Vec::new(), |mut names, binding| { if !names.iter().any(|(label, _)| label == &binding.0) { names.push(binding); } names });
        (!names.is_empty()).then_some((home, names))
    }

    fn direct_eval_context(&mut self, p: &ResidualProgram) -> Option<crate::compile::EvalContext> {
        if !self.direct_eval {
            return None;
        }
        let frame = self.frames.len().checked_sub(1)?;
        let home_atom = p
            .function_get(self.frames[frame].function as usize)?
            .super_home_atom;
        let class_initializer = self.in_class_initializer(p);
        let atom = self.runtime_atoms.lexical_this;
        let super_calls = home_atom.is_some()
            && !class_initializer
            && self
                .lexical_this_owner(frame, atom)
                .and_then(|(program, function, _)| {
                    self.programs.get(program).and_then(|program| {
                        program
                            .function_get(function as usize)
                            .map(|f| f.derived_constructor)
                    })
                })
                .unwrap_or(false);
        Some(crate::compile::EvalContext {
            in_function: self.direct_eval_function_context(p),
            home_atom,
            super_calls,
            class_initializer,
        })
    }

    fn eval_home_binding(&mut self, p: &ResidualProgram) -> Option<Value> {
        let function = self.frames.last()?.function;
        let home_atom = p.function_get(function as usize)?.super_home_atom?;
        // Use the same binding operation as compiled super access. Its retained
        // metadata resolves captures across both scope views and eval programs.
        self.load_name(p, home_atom, None).ok()
    }

    fn direct_eval_binding(&self, atom: Atom) -> Option<crate::bytecode::EvalBinding> {
        self.name_binding(self.frames.len().checked_sub(1)?, atom)
    }

    fn prepare_direct_eval_var_bindings(&mut self, atoms: &[Atom]) {
        let Some(frame_index) = self
            .frames
            .len()
            .checked_sub(1)
            .map(|frame| self.variable_environment_frame(frame))
        else {
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
        let text = self.heap.alloc_string(message.into());
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
        let key = self.heap.alloc_string(self.atom_value(atom));
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

#[cfg(test)]
mod tests {
    use super::only_trivia;

    #[test]
    fn trivia_scanner_accepts_ecmascript_unicode_space_and_rejects_other_text() {
        assert!(only_trivia("\u{FEFF}\u{2003}/* comment */\u{2028}// line comment"));
        assert!(!only_trivia("\u{0085}"));
        assert!(!only_trivia("/* unterminated"));
        assert!(!only_trivia("1"));
    }
}
