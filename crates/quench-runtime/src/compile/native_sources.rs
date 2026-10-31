//! Compile source units into one stable atom namespace before host initialization.

use super::{AtomPrefix, Diagnostic, HostScriptOptions, ReleasedRecords, DynamicFunctionKind, DynamicFunctionSource, DYNAMIC_FUNCTION_GOAL, DYNAMIC_FUNCTION_SOURCE_NAME, STATIC_MODULE_GOAL, STATIC_TYPESCRIPT_MODULE_GOAL};
use crate::bytecode::{Atom, AtomSequence, AtomTable, SourceText};
use crate::{Engine, ResidualProgram};
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// Exact host source identity, including the compilation shape. Hashes may index
/// this key, but equality always checks the original strings.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeSourceKey {
    name: String,
    parts: Vec<String>,
}

impl NativeSourceKey {
    pub fn new(name: &str, parts: &[&str]) -> Result<Self, String> {
        match parts {
            ["specialized" | "unspecialized", _] => {},
            ["specialized" | "unspecialized", STATIC_MODULE_GOAL | STATIC_TYPESCRIPT_MODULE_GOAL, _] => {},
            ["specialized" | "unspecialized", goal, _] => { script_options(goal)?; },
            ["specialized" | "unspecialized", DYNAMIC_FUNCTION_GOAL, kind, _, _]
                if DynamicFunctionKind::from_source_prefix(kind).is_some() => {},
            ["specialized" | "unspecialized", _, _, _] => {},
            _ => return Err("unknown native host source key".into()),
        }
        Ok(Self { name: name.into(), parts: parts.iter().map(|part| (*part).into()).collect() })
    }

    pub fn function_constructor(specialized: bool, kind: DynamicFunctionKind, parameters: &str, body: &str) -> Self {
        let mode = if specialized { "specialized" } else { "unspecialized" };
        Self { name: DYNAMIC_FUNCTION_SOURCE_NAME.into(), parts: kind.native_parts(mode, parameters, body).into_iter().map(str::to_owned).collect() }
    }

    pub fn is_function_constructor(&self) -> bool {
        matches!(self.parts.as_slice(), [_, goal, _, _, _] if goal == DYNAMIC_FUNCTION_GOAL)
    }

    pub fn name(&self) -> &str { &self.name }
    pub fn parts(&self) -> impl ExactSizeIterator<Item = &str> { self.parts.iter().map(String::as_str) }
}

fn script_options(goal: &str) -> Result<HostScriptOptions, String> {
    let invalid = || "invalid native host Script goal".to_owned();
    let fields: Vec<_> = goal.strip_prefix("specialized-script:").ok_or_else(invalid)?.split(':').collect();
    let (line, column, kept, typescript) = match fields.as_slice() {
        [line, column, kept] => (line, column, kept, false),
        [line, column, kept, "typescript"] => (line, column, kept, true),
        _ => return Err(invalid()),
    };
    let options = HostScriptOptions {
        origin: Some((line.parse().map_err(|_| invalid())?, column.parse().map_err(|_| invalid())?)),
        keep_records: kept.parse().map_err(|_| invalid())?,
        typescript,
    };
    if options.cache_goal() != goal { return Err(invalid()); }
    Ok(options)
}

/// Source validation produces either code or a deferred guest syntax failure.
pub enum NativeSourceCompilation {
    Program(ResidualProgram),
    SyntaxError(String),
}

/// A compilation's append-only namespace. All emitted images use these IDs;
/// `finish` seeds the entry with the full namespace before the host can add names.
pub struct NativeJsCompiler {
    atoms: Rc<AtomSequence>,
    index: FxHashMap<Rc<str>, Atom>,
    specialized: bool,
}

impl NativeJsCompiler {
    pub fn new(specialized: bool) -> Self {
        Self {
            atoms: Rc::new(AtomSequence::new(AtomTable::default())),
            index: FxHashMap::default(),
            specialized,
        }
    }

    /// Compile a captured loader input without executing its guest code.
    pub fn compile_source(&mut self, key: &NativeSourceKey) -> Result<ResidualProgram, String> {
        let parts: Vec<_> = key.parts().collect();
        let expected = if self.specialized { "specialized" } else { "unspecialized" };
        if parts.first().copied() != Some(expected) {
            return Err("native source specialization does not match compiler".into());
        }
        match parts.as_slice() {
            [_, source] => self.compile_host_script(source, key.name()).map_err(|errors| format!("{errors:?}")),
            [_, goal @ (STATIC_MODULE_GOAL | STATIC_TYPESCRIPT_MODULE_GOAL), source] => self.compile_module_with_syntax(source, key.name(), *goal == STATIC_TYPESCRIPT_MODULE_GOAL)
                .map_err(|errors| format!("{errors:?}")),
            [_, goal, source] => self.compile_script(source, key.name(), script_options(goal)?)
                .map_err(|errors| format!("{errors:?}")),
            [_, DYNAMIC_FUNCTION_GOAL, kind, parameters, body] => {
                let kind = DynamicFunctionKind::from_source_prefix(kind).ok_or("invalid native Function kind")?;
                self.compile_function_constructor(kind, parameters, body).map_err(|errors| format!("{errors:?}"))
            },
            [_, prefix, body, suffix] => self.compile_library_body(prefix, std::sync::Arc::from(*body), suffix, key.name()),
            _ => Err("unknown native host source key".into()),
        }
    }

    pub fn compile_function_constructor(
        &mut self, kind: DynamicFunctionKind, parameters: &str, body: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let source = DynamicFunctionSource::new(parameters, body, kind);
        let mut program = Engine::specialize_function_constructor(&source, DYNAMIC_FUNCTION_SOURCE_NAME, self)?;
        self.adopt(&mut program);
        Ok(program)
    }

    /// A Function request keeps ECMAScript syntax failures as data until invocation.
    pub fn compile_function_source(&mut self, key: &NativeSourceKey) -> Result<NativeSourceCompilation, String> {
        let parts: Vec<_> = key.parts().collect();
        let [mode, DYNAMIC_FUNCTION_GOAL, kind, parameters, body] = parts.as_slice() else {
            return Err("native source is not a Function request".into());
        };
        if *mode != if self.specialized { "specialized" } else { "unspecialized" } {
            return Err("native source specialization does not match compiler".into());
        }
        let kind = DynamicFunctionKind::from_source_prefix(kind).ok_or("invalid native Function kind")?;
        match self.compile_function_constructor(kind, parameters, body) {
            Ok(program) => Ok(NativeSourceCompilation::Program(program)),
            Err(errors) if !errors.is_empty() && errors.iter().all(Diagnostic::is_syntax_error) => {
                Ok(NativeSourceCompilation::SyntaxError(errors[0].syntax_message().to_owned()))
            },
            Err(errors) => Err(format!("{errors:?}")),
        }
    }

    pub fn compile_script(
        &mut self, source: &str, name: &str, options: HostScriptOptions,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let mut program = Engine::specialize_host_script(
            source, name, self, options, ReleasedRecords::Dropped,
        )?;
        if let (Some(source), Some((line, column))) = (&mut program.source, options.origin) {
            source.line_offset = line;
            source.column_offset = column;
        }
        self.adopt(&mut program);
        Ok(program)
    }

    /// Compile a module in the same namespace as its precompiled host scripts.
    pub fn compile_module(&mut self, source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        self.compile_module_with_syntax(source, name, false)
    }

    fn compile_module_with_syntax(&mut self, source: &str, name: &str, typescript: bool) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let mut program = Engine::specialize_module_with_atom_prefix(source, name, typescript, self, ReleasedRecords::Dropped)?;
        self.adopt(&mut program);
        Ok(program)
    }

    /// Retain a frozen loader source without another resident copy of its text.
    pub fn compile_module_source(&mut self, module: &crate::ModuleSource) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let mut program = self.compile_module_with_syntax(&module.source, &module.name, module.typescript)?;
        if let Some(source) = &mut program.source {
            source.text = SourceText::Shared(module.source.clone());
        }
        Ok(program)
    }

    /// Compile context-free host bootstrap source, matching `eval_host_script`.
    pub fn compile_host_script(
        &mut self, source: &str, name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let mut program = Engine::specialize_eval_with_context(
            source, name, self, false, Default::default(), &[], &[],
        )?;
        self.adopt(&mut program);
        Ok(program)
    }

    /// Compile the same retained wrapper/body shape used by host libraries.
    pub fn compile_library_body(
        &mut self, prefix: &str, body: std::sync::Arc<str>, suffix: &str, name: &str,
    ) -> Result<ResidualProgram, String> {
        let origin = crate::api::wrapper_origin(prefix)
            .ok_or("host wrapper origin exceeds compiler positions")?;
        let base = u32::try_from(prefix.len())
            .map_err(|_| "host wrapper prefix exceeds compiler positions")?;
        let wrapped = [prefix, &body, suffix].concat();
        let mut program = self.compile_script(&wrapped, name, HostScriptOptions {
            origin: Some(origin), keep_records: true, typescript: false,
        }).map_err(|diagnostics| format!("{diagnostics:?}"))?;
        if let Some(source) = &mut program.source {
            source.text = SourceText::Shared(body);
            source.base = base;
        }
        Ok(program)
    }

    fn adopt(&mut self, program: &mut ResidualProgram) {
        program.compile_released();
        for id in self.atoms.len()..program.atoms.len() {
            let name: Rc<str> = Rc::from(&program.atoms[id]);
            self.atoms.append(crate::vm::wtf16::JsString::from_str(&name));
            self.index.insert(name, id as Atom);
        }
        program.atoms = AtomTable::shared(self.atoms.clone(), self.atoms.len());
    }

    /// Seal the namespace. The entry must have been compiled by this compiler.
    /// Emit its native image after this step; all other images retain their IDs.
    pub fn finish(self, mut entry: ResidualProgram) -> Result<ResidualProgram, String> {
        if !entry.atoms.belongs_to(&self.atoms) {
            return Err("native entry belongs to a different compiler namespace".into());
        }
        let len = self.atoms.len();
        entry.atoms = AtomTable::shared(self.atoms, len);
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_units_share_ids_and_entry_seeds_names_from_later_units() {
        let mut compiler = NativeJsCompiler::new(true);
        let options = HostScriptOptions::default();
        let first = compiler.compile_script("globalThis.first = 1;", "first.js", options).unwrap();
        let second = compiler.compile_script("globalThis.second = globalThis.first;", "second.js", options).unwrap();
        assert!(first.atoms.len() < second.atoms.len());
        assert!(first.atoms.iter().zip(second.atoms.iter()).all(|(a, b)| a == b));
        let entry = compiler.finish(first).unwrap();
        assert_eq!(entry.atoms.iter().collect::<Vec<_>>(), second.atoms.iter().collect::<Vec<_>>());
        entry.validate().unwrap();
        second.validate().unwrap();
    }

    #[test]
    fn sealing_rejects_an_independently_compiled_entry() {
        let entry = Engine::specialize("", "foreign.js").unwrap();
        assert!(NativeJsCompiler::new(true).finish(entry).is_err());
    }
}

impl AtomPrefix for NativeJsCompiler {
    fn len(&self) -> usize { self.atoms.len() }
    fn name(&self, atom: Atom) -> &str { self.atoms.name(atom as usize) }
    fn find(&self, name: &str) -> Option<Atom> { self.index.get(name).copied() }
    fn table(&self) -> AtomTable { AtomTable::shared(self.atoms.clone(), self.atoms.len()) }
    fn specialized(&self) -> bool { self.specialized }
}

impl Engine {
    /// Potential constructor inputs, never proof of the callee's identity. The
    /// actual intrinsic performs an exact key lookup after argument conversion.
    pub fn literal_function_constructor_sources(program: &oxc_ast::ast::Program<'_>, specialized: bool) -> Vec<NativeSourceKey> {
        use oxc_ast::ast::{Argument, CallExpression, Expression, NewExpression};
        use oxc_ast_visit::{Visit, walk};
        struct Requests { specialized: bool, keys: std::collections::BTreeSet<NativeSourceKey> }
        impl Requests {
            fn record(&mut self, callee: &Expression<'_>, arguments: &[Argument<'_>]) {
                let kind = match callee.get_inner_expression() {
                    Expression::Identifier(name) if name.name == "Function" => DynamicFunctionKind::Ordinary,
                    Expression::StaticMemberExpression(member) if member.property.name == "Function"
                        && matches!(member.object.get_inner_expression(), Expression::Identifier(name) if name.name == "globalThis") => DynamicFunctionKind::Ordinary,
                    Expression::StaticMemberExpression(member) if member.property.name == "constructor" => {
                        let Expression::FunctionExpression(function) = member.object.get_inner_expression() else { return; };
                        match (function.r#async, function.generator) {
                            (false, false) => DynamicFunctionKind::Ordinary,
                            (true, false) => DynamicFunctionKind::Async,
                            (false, true) => DynamicFunctionKind::Generator,
                            (true, true) => DynamicFunctionKind::AsyncGenerator,
                        }
                    },
                    _ => return,
                };
                let Some(mut strings) = arguments.iter().map(|argument| {
                    match argument.as_expression()?.get_inner_expression() {
                        Expression::StringLiteral(text) => Some(text.value.as_str()),
                        _ => None,
                    }
                }).collect::<Option<Vec<_>>>() else { return; };
                let body = strings.pop().unwrap_or_default();
                self.keys.insert(NativeSourceKey::function_constructor(self.specialized, kind, &strings.join(","), body));
            }
        }
        impl<'a> Visit<'a> for Requests {
            fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
                self.record(&call.callee, &call.arguments);
                walk::walk_call_expression(self, call);
            }
            fn visit_new_expression(&mut self, call: &NewExpression<'a>) {
                self.record(&call.callee, &call.arguments);
                walk::walk_new_expression(self, call);
            }
        }
        let mut requests = Requests { specialized, keys: Default::default() };
        requests.visit_program(program);
        requests.keys.into_iter().collect()
    }
}
