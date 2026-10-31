//! Compile source units into one stable atom namespace before host initialization.

use super::{AtomPrefix, Diagnostic, HostScriptOptions, ReleasedRecords, DynamicFunctionKind, DynamicFunctionSource, DYNAMIC_FUNCTION_GOAL, DYNAMIC_FUNCTION_SOURCE_NAME, STATIC_MODULE_GOAL, STATIC_TYPESCRIPT_MODULE_GOAL};
use crate::bytecode::{Atom, AtomSequence, AtomTable, SourceText};
use crate::{Engine, ResidualProgram};
use rustc_hash::FxHashMap;
use std::{cell::RefCell, collections::{BTreeMap, BTreeSet}, rc::Rc};

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
    function_requests: Option<RefCell<BTreeSet<NativeSourceKey>>>,
}

impl NativeJsCompiler {
    pub fn new(specialized: bool) -> Self {
        Self {
            atoms: Rc::new(AtomSequence::new(AtomTable::default())),
            index: FxHashMap::default(),
            specialized,
            function_requests: None,
        }
    }

    /// Discover literal constructor obligations while compiling validated syntax.
    pub fn with_function_source_discovery(mut self) -> Self {
        self.function_requests.get_or_insert_with(|| RefCell::new(BTreeSet::new()));
        self
    }

    /// Compile the transitive literal-constructor graph before namespace sealing.
    pub fn compile_function_sources(&mut self) -> Result<BTreeMap<NativeSourceKey, NativeSourceCompilation>, String> {
        let mut compiled = BTreeMap::new();
        loop {
            let key = self.function_requests.as_mut().ok_or("native Function discovery is disabled")?.get_mut().pop_first();
            let Some(key) = key else { break; };
            if compiled.contains_key(&key) { continue; }
            let artifact = match self.compile_function_source(&key) {
                Ok(artifact) => artifact,
                Err(error) => {
                    let pending = self.function_requests.as_mut().expect("discovery is enabled").get_mut();
                    pending.insert(key);
                    pending.extend(compiled.into_keys());
                    return Err(error);
                },
            };
            compiled.insert(key, artifact);
        }
        Ok(compiled)
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
        if self.function_requests.as_ref().is_some_and(|pending| !pending.borrow().is_empty()) {
            return Err("native Function obligations must be compiled before namespace sealing".into());
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
    fn discovered_function_graph_is_deduplicated_and_defers_invalid_source() {
        let mut compiler = NativeJsCompiler::new(false).with_function_source_discovery();
        let entry = compiler.compile_script(r#"
            Function("return Function('return 42;');");
            Function("return Function('return 42;');");
            if (false) Function('let invalid = ;');
            if (false) Function("Function('void 1;'); let invalid = ;");
        "#, "entry.js", Default::default()).unwrap();
        let sources = compiler.compile_function_sources().unwrap();
        assert_eq!(sources.values().filter(|source| matches!(source, NativeSourceCompilation::Program(_))).count(), 2);
        assert_eq!(sources.values().filter(|source| matches!(source, NativeSourceCompilation::SyntaxError(_))).count(), 2);
        assert!(!sources.contains_key(&NativeSourceKey::function_constructor(false, DynamicFunctionKind::Ordinary, "", "void 1;")));
        let entry = compiler.finish(entry).unwrap();
        for source in sources.values() {
            if let NativeSourceCompilation::Program(program) = source {
                assert!(program.atoms.iter().zip(entry.atoms.iter()).all(|(a, b)| a == b));
                program.validate().unwrap();
            }
        }
    }

    #[test]
    fn namespace_sealing_requires_discovered_obligations_and_failed_parses_add_none() {
        let mut compiler = NativeJsCompiler::new(false).with_function_source_discovery();
        let entry = compiler.compile_script("Function('return 42;');", "entry.js", Default::default()).unwrap();
        assert!(compiler.with_function_source_discovery().finish(entry).unwrap_err().contains("obligations"));
        let mut compiler = NativeJsCompiler::new(false).with_function_source_discovery();
        let entry = compiler.compile_script("", "entry.js", Default::default()).unwrap();
        assert!(compiler.compile_script("Function('return 1;'); const invalid = ;", "invalid.js", Default::default()).is_err());
        assert!(compiler.compile_function_sources().unwrap().is_empty());
        compiler.finish(entry).unwrap();
    }

    #[test]
    fn failed_function_graph_compilation_keeps_all_obligations_pending() {
        let mut compiler = NativeJsCompiler::new(true).with_function_source_discovery();
        let entry = compiler.compile_script("Function('return 42;');", "entry.js", Default::default()).unwrap();
        let valid = NativeSourceKey::function_constructor(true, DynamicFunctionKind::Ordinary, "", "return 42;");
        let incompatible = NativeSourceKey::function_constructor(false, DynamicFunctionKind::Ordinary, "", "return 0;");
        compiler.function_requests.as_mut().unwrap().get_mut().insert(incompatible.clone());
        assert!(compiler.compile_function_sources().is_err());
        {
            let pending = compiler.function_requests.as_ref().unwrap().borrow();
            assert!(pending.contains(&valid) && pending.contains(&incompatible));
        }
        assert!(compiler.finish(entry).unwrap_err().contains("obligations"));
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
    fn native_function_requests(&self) -> Option<&RefCell<BTreeSet<NativeSourceKey>>> { self.function_requests.as_ref() }
}

mod constructors;

impl Engine {
    /// Literal arguments through possible constructor bindings. Actual intrinsic
    /// identity and exact source input are still guarded at invocation.
    pub fn literal_function_constructor_sources(program: &oxc_ast::ast::Program<'_>, specialized: bool) -> Vec<NativeSourceKey> {
        let semantic = oxc_semantic::SemanticBuilder::new().build(program);
        Self::function_constructor_sources(program, semantic.semantic.scoping(), specialized)
    }

    /// Discover literal constructor obligations using the caller's authoritative
    /// OXC scopes, shared with its other source analyses.
    pub fn function_constructor_sources(program: &oxc_ast::ast::Program<'_>, scoping: &oxc_semantic::Scoping, specialized: bool) -> Vec<NativeSourceKey> {
        constructors::sources(program, scoping, specialized)
    }
}
