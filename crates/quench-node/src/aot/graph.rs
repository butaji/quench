//! Conservative, syntax-owned module discovery. Unresolved edges are retained;
//! discovery never executes module initialization or promises a closed graph.

use super::{SourceManifest, ModuleFile};
use crate::host::shared_vm::commonjs;
use oxc_ast::ast::{CallExpression, ComputedMemberExpression, Expression, IdentifierReference, ImportExpression, LogicalExpression, StaticMemberExpression, UnaryExpression};
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}};

mod loader_bindings;
use loader_bindings::{Base, LoaderBindings};

#[derive(Clone, Copy)]
enum RequestKind { Resolve, Load }

#[derive(Debug)]
pub enum GraphRequirement {
    DynamicRequire { from: PathBuf, offset: u32 },
    DynamicImport { from: PathBuf, offset: u32 },
    Unresolved { from: PathBuf, specifier: String, diagnostic: String },
    NativeAddon { path: PathBuf },
    Wasm { path: PathBuf },
}

impl GraphRequirement {
    pub fn report(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Self::DynamicRequire { from, offset } => json!({"kind": "dynamic_require", "from": from, "offset": offset}),
            Self::DynamicImport { from, offset } => json!({"kind": "dynamic_import", "from": from, "offset": offset}),
            Self::Unresolved { from, specifier, diagnostic } => json!({"kind": "unresolved", "from": from, "specifier": specifier, "diagnostic": diagnostic}),
            Self::NativeAddon { path } => json!({"kind": "native_addon", "path": path}),
            Self::Wasm { path } => json!({"kind": "wasm", "path": path}),
        }
    }
}

#[derive(Default)]
pub struct DependencyGraph {
    pub files: BTreeMap<PathBuf, ModuleFile>,
    pub resolutions: BTreeMap<(PathBuf, String), PathBuf>,
    pub builtins: BTreeSet<String>,
    pub esm_builtins: BTreeSet<String>,
    pub host_globals: BTreeSet<String>,
    pub requirements: Vec<GraphRequirement>,
    /// Unshadowed eval references may receive source first known at execution.
    pub dynamic_eval_sites: Vec<(PathBuf, u32)>,
    /// Calls whose possible Function-constructor identity receives nonliteral source.
    pub dynamic_function_sites: Vec<(PathBuf, u32)>,
}

struct Requests<'s> {
    scoping: &'s oxc_semantic::Scoping,
    loaders: &'s LoaderBindings<'s>,
    specifiers: BTreeMap<String, RequestKind>,
    classified_requires: BTreeSet<u32>,
    dynamic_requires: BTreeSet<u32>,
    dynamic_imports: BTreeSet<u32>,
    literal_imports: BTreeSet<String>,
    host_globals: BTreeSet<String>,
    dynamic_eval_sites: BTreeSet<u32>,
}

impl<'s> Requests<'s> {
    fn new(scoping: &'s oxc_semantic::Scoping, loaders: &'s LoaderBindings<'s>) -> Self {
        Self { scoping, loaders, specifiers: BTreeMap::new(), classified_requires: loaders.transfers(),
            dynamic_requires: BTreeSet::new(), dynamic_imports: BTreeSet::new(),
            literal_imports: BTreeSet::new(), host_globals: BTreeSet::new(), dynamic_eval_sites: BTreeSet::new() }
    }

    fn request(&mut self, specifier: String, kind: RequestKind) {
        self.specifiers.entry(specifier).and_modify(|existing| {
            if matches!(kind, RequestKind::Load) { *existing = kind; }
        }).or_insert(kind);
    }

    fn runtime_reference(&self, name: &IdentifierReference<'_>) -> Option<&oxc_syntax::reference::Reference> {
        let reference = self.scoping.get_reference(name.reference_id.get()?);
        (reference.is_value() && !reference.flags().is_value_as_type()).then_some(reference)
    }

    fn is_global_object(&self, expression: &Expression<'_>) -> bool {
        matches!(expression.get_inner_expression(), Expression::Identifier(name)
            if name.name == "globalThis" && self.runtime_reference(name).is_some_and(|reference|reference.symbol_id().is_none()))
    }

    fn record_host_global(&mut self, name: &str) {
        if name == "console" { self.request("node:console".into(), RequestKind::Load); }
        let interfaces = commonjs::WEB_STREAM_GLOBALS.iter().chain(commonjs::WEB_API_GLOBALS).chain(commonjs::FETCH_API_GLOBALS).copied()
            .chain(crate::modules::events_shared_vm::EVENT_GLOBALS.iter().copied())
            .chain(crate::modules::events_shared_vm::MESSAGING_GLOBALS.iter().copied())
            .chain(crate::modules::text_decoder_shared_vm::GLOBAL_NAMES.iter().copied())
            .chain(crate::modules::util_lazy_shared_vm::lazy_global_names());
        if name == "performance" || interfaces.into_iter().any(|global|global == name) {
            self.host_globals.insert(name.to_owned());
        }
    }
}

impl<'a> Visit<'a> for Requests<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(require) = self.loaders.require_call(call) {
            if let Some(reference) = require.reference { self.classified_requires.insert(reference); }
            let kind = if require.resolve_only { RequestKind::Resolve } else { RequestKind::Load };
            match require.arguments {
                [argument] => match argument.as_expression() {
                    Some(expression) => match expression.get_inner_expression() {
                        Expression::StringLiteral(name) if require.base == Base::Current || commonjs::is_builtin_specifier(name.value.as_str()) => {
                            self.request(name.value.to_string(), kind);
                        },
                        _ => { self.dynamic_requires.insert(call.span.start); },
                    },
                    _ => { self.dynamic_requires.insert(call.span.start); },
                },
                _ => { self.dynamic_requires.insert(call.span.start); },
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_identifier_reference(&mut self, name: &IdentifierReference<'a>) {
        let Some(reference) = self.runtime_reference(name) else { return };
        let global = reference.symbol_id().is_none();
        if global { self.record_host_global(name.name.as_str()); }
        if global && name.name == "eval" { self.dynamic_eval_sites.insert(name.span.start); }
        if self.loaders.is_require_reference(name) && !self.classified_requires.contains(&name.span.start) {
            self.dynamic_requires.insert(name.span.start);
        }
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        if self.is_global_object(&member.object) {
            self.record_host_global(member.property.name.as_str());
            if member.property.name == "eval" { self.dynamic_eval_sites.insert(member.span.start); }
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        if let (true, Expression::StringLiteral(name)) =
            (self.is_global_object(&member.object), member.expression.get_inner_expression()) {
            self.record_host_global(name.value.as_str());
            if name.value == "eval" { self.dynamic_eval_sites.insert(member.span.start); }
        }
        walk::walk_computed_member_expression(self, member);
    }

    fn visit_logical_expression(&mut self, expression: &LogicalExpression<'a>) {
        if expression.operator == oxc_syntax::operator::LogicalOperator::And {
            if let Expression::Identifier(name) = expression.left.get_inner_expression() {
                if self.loaders.is_require_reference(name) {
                    self.classified_requires.insert(name.span.start);
                }
            }
        }
        walk::walk_logical_expression(self, expression);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        match (expression.operator, expression.argument.get_inner_expression()) {
            (oxc_syntax::operator::UnaryOperator::Typeof, Expression::Identifier(name)) if self.loaders.is_require_reference(name) => {
                self.classified_requires.insert(name.span.start);
            },
            _ => {},
        }
        walk::walk_unary_expression(self, expression);
    }

    fn visit_import_expression(&mut self, import: &ImportExpression<'a>) {
        match (import.phase, import.source.get_inner_expression()) {
            (None, Expression::StringLiteral(specifier)) => { self.literal_imports.insert(specifier.value.to_string()); },
            _ => { self.dynamic_imports.insert(import.span().start); },
        }
        walk::walk_import_expression(self, import);
    }
}

pub(super) fn discover(manifest: &mut SourceManifest, entry: &Path) -> Result<DependencyGraph, String> {
    use std::{cell::RefCell, rc::Rc};
    use quench_runtime::{Host, ModuleSource, SourceKind};
    let entry = entry.canonicalize().map_err(|error| error.to_string())?;
    let assets = Rc::new(RefCell::new(super::ModuleAssets::default()));
    let mut host = crate::NodeHost::new(vec!["quench".into()]).with_exec_argv(Vec::new())?;
    host.module_assets = super::ModuleAssetState::Capture(assets.clone());
    let mut pending_files = BTreeSet::from([entry.clone()]);
    let mut seen_files = BTreeSet::new();
    let mut pending_modules = BTreeMap::<String, ModuleSource>::new();
    let mut seen_modules = BTreeSet::new();
    let mut graph = DependencyGraph::default();
    while !pending_files.is_empty() || !pending_modules.is_empty() {
        let (name, text, source_type) = if let Some(path) = pending_files.pop_first() {
            if !seen_files.insert(path.clone()) { continue; }
            let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let extension = path.extension().and_then(|name| name.to_str());
            let kind = match extension {
                Some("json" | "node") => SourceKind::Script,
                Some("wasm") => SourceKind::Module,
                _ => commonjs::source_kind(&path)?,
            };
            host.record_module_bytes(&path, kind, &bytes).map_err(|error| error.to_string())?;
            match extension {
                Some("json") => continue,
                Some("node") => { graph.requirements.push(GraphRequirement::NativeAddon { path }); continue; },
                Some("wasm") => { graph.requirements.push(GraphRequirement::Wasm { path }); continue; },
                _ => {},
            }
            if kind == SourceKind::Module {
                let url = url::Url::from_file_path(&path).map_err(|_| "invalid module file URL")?;
                let referrer = if path == entry { super::compiler::NATIVE_ENTRY_NAME }
                    else { path.to_str().ok_or("module path is not UTF-8")? };
                let module = host.resolve_dynamic_import(referrer, url.as_str()).map_err(|error| error.message)?
                    .ok_or("module entry was not resolved")?;
                pending_modules.insert(module.name.clone(), module);
                continue;
            }
            if matches!(extension, Some("ts" | "cts")) {
                let text = commonjs::source_text(&bytes);
                let projected = quench_runtime::Engine::project_typescript_module_syntax(&text).map(|(source, _)| source).unwrap_or_else(|| text.clone());
                host.record_prepared_module(&path, &projected)?;
            }
            manifest.insert_key(commonjs::native_commonjs_source(&path, &bytes, path == entry)?);
            (path.to_string_lossy().into_owned(), commonjs::source_text(&bytes).into_owned(),
                oxc_span::SourceType::script().with_typescript(matches!(extension, Some("ts" | "cts" | "mts"))))
        } else {
            let (name, module) = pending_modules.pop_first().expect("pending graph node");
            if !seen_modules.insert(name.clone()) { continue; }
            let path = crate::host::shared_vm::referrer_location(&name).map_err(|error| error.message)?.1;
            if path.extension().is_some_and(|extension| extension == "json") { continue; }
            manifest.insert(&name, &["specialized", module.compilation_goal(), &module.source])?;
            (name, module.source.to_string(), oxc_span::SourceType::mjs().with_typescript(module.typescript))
        };
        let body = commonjs::script_body(&text);
        let base = u32::try_from(text.len() - body.len()).map_err(|_| "module source exceeds compiler positions")?;
        let allocator = oxc_allocator::Allocator::default();
        let mut parsed = oxc_parser::Parser::new(&allocator, body, source_type)
            .with_options(oxc_parser::ParseOptions { allow_return_outside_function: source_type.is_script(), ..Default::default() }).parse();
        if !parsed.diagnostics.is_empty() { return Err(format!("{name}: {:?}", parsed.diagnostics)); }
        quench_runtime::Engine::erase_type_only_module_syntax(&mut parsed.program);
        let semantic = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        let scoping = semantic.semantic.scoping();
        let (constructor_sources, dynamic_functions) = quench_runtime::Engine::function_constructor_source_demands(&parsed.program, scoping, true);
        for key in constructor_sources {
            manifest.insert_key(key);
        }
        let path = crate::host::shared_vm::referrer_location(&name).map_err(|error| error.message)?.1;
        graph.dynamic_function_sites.extend(dynamic_functions.into_iter().map(|offset| (path.clone(), offset + base)));
        let loaders = LoaderBindings::discover(&parsed.program, scoping, &path);
        let mut requests = Requests::new(scoping, &loaders);
        requests.visit_program(&parsed.program);
        graph.host_globals.extend(std::mem::take(&mut requests.host_globals));
        graph.dynamic_eval_sites.extend(requests.dynamic_eval_sites.into_iter().map(|offset| (path.clone(), offset + base)));
        for offset in requests.dynamic_requires {
            graph.requirements.push(GraphRequirement::DynamicRequire { from: name.clone().into(), offset: offset + base });
        }
        for offset in requests.dynamic_imports {
            graph.requirements.push(GraphRequirement::DynamicImport { from: name.clone().into(), offset: offset + base });
        }
        for (specifier, kind) in requests.specifiers {
            if commonjs::is_builtin_specifier(&specifier) {
                if matches!(kind, RequestKind::Load) { graph.builtins.insert(specifier); }
                continue;
            }
            let directory = path.parent().ok_or("module has no parent directory")?;
            let resolved = commonjs::with_commonjs_resolver(|resolver| commonjs::resolve_kept(resolver, |resolver| resolver.resolve(directory, &specifier)));
            match resolved {
                Ok(resolved) => {
                    let target = resolved.full_path().canonicalize().map_err(|error| error.to_string())?;
                    host.record_module_resolution(&path, &specifier, &target)?;
                    if matches!(kind, RequestKind::Load) { pending_files.insert(target); }
                },
                Err(error) => graph.requirements.push(GraphRequirement::Unresolved {
                    from: name.clone().into(), specifier, diagnostic: error.to_string(),
                }),
            }
        }
        let imports: BTreeSet<_> = quench_runtime::Engine::module_requests(&parsed.program).into_iter()
            .map(|request| request.source).chain(requests.literal_imports).collect();
        for specifier in imports {
            let specifier = specifier.as_str();
            if commonjs::is_builtin_specifier(specifier) {
                graph.esm_builtins.insert(format!("node:{}", specifier.strip_prefix("node:").unwrap_or(specifier)));
            }
            match host.resolve_dynamic_import(&name, specifier) {
                Ok(Some(module)) => { pending_modules.insert(module.name.clone(), module); },
                Ok(None) => graph.requirements.push(GraphRequirement::Unresolved {
                    from: name.clone().into(), specifier: specifier.to_string(), diagnostic: "module request was not resolved".into(),
                }),
                Err(error) => graph.requirements.push(GraphRequirement::Unresolved {
                    from: name.clone().into(), specifier: specifier.to_string(), diagnostic: error.message,
                }),
            }
        }
        // Import interop may discover CommonJS inputs. Their ordinary wrappers
        // and require edges still compile through the same file lifecycle.
        pending_files.extend(assets.borrow().files.keys().filter(|path| !seen_files.contains(*path)).cloned());
    }
    drop(host);
    let assets = Rc::try_unwrap(assets).map_err(|_| "module graph assets are still borrowed")?.into_inner();
    graph.files = assets.files.clone();
    graph.resolutions = assets.resolutions.clone();
    manifest.assets.extend(assets)?;
    Ok(graph)
}
