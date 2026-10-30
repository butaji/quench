//! Conservative, syntax-owned module discovery. Unresolved edges are retained;
//! discovery never executes module initialization or promises a closed graph.

use super::{SourceManifest, ModuleFile};
use crate::host::shared_vm::commonjs;
use oxc_ast::ast::{CallExpression, Expression, IdentifierReference, ImportExpression};
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}};

#[derive(Debug)]
pub enum GraphRequirement {
    DynamicRequire { from: PathBuf, offset: u32 },
    DynamicImport { from: PathBuf, offset: u32 },
    Unresolved { from: PathBuf, specifier: String, diagnostic: String },
    EsModule { path: PathBuf },
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
            Self::EsModule { path } => json!({"kind": "es_module", "path": path}),
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
    pub host_globals: BTreeSet<String>,
    pub requirements: Vec<GraphRequirement>,
}

#[derive(Default)]
struct Requests {
    specifiers: BTreeSet<String>,
    direct_requires: BTreeSet<u32>,
    dynamic_requires: BTreeSet<u32>,
    dynamic_imports: BTreeSet<u32>,
    host_globals: BTreeSet<String>,
}

impl<'a> Visit<'a> for Requests {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let require = match &call.callee {
            Expression::Identifier(name) if name.name == "require" => {
                self.direct_requires.insert(name.span.start);
                true
            },
            Expression::StaticMemberExpression(member) if member.property.name == "require" => {
                matches!(&member.object, Expression::Identifier(name) if name.name == "module")
            },
            _ => false,
        };
        if require {
            match call.arguments.as_slice() {
                [argument] => match argument.as_expression() {
                    Some(Expression::StringLiteral(name)) => { self.specifiers.insert(name.value.to_string()); },
                    _ => { self.dynamic_requires.insert(call.span.start); },
                },
                _ => { self.dynamic_requires.insert(call.span.start); },
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_identifier_reference(&mut self, name: &IdentifierReference<'a>) {
        // The global facade delegates rich values and formatting to the same SDK
        // module as require('node:console'), even when an entry only uses writers.
        if name.name == "console" {
            self.specifiers.insert("node:console".into());
        }
        if commonjs::WEB_STREAM_GLOBALS.iter().chain(commonjs::WEB_API_GLOBALS).chain(commonjs::FETCH_API_GLOBALS)
            .any(|global| *global == name.name.as_str()) {
            self.host_globals.insert(name.name.to_string());
        }
        if name.name == "require" && !self.direct_requires.contains(&name.span.start) {
            self.dynamic_requires.insert(name.span.start);
        }
    }

    fn visit_import_expression(&mut self, import: &ImportExpression<'a>) {
        self.dynamic_imports.insert(import.span().start);
        walk::walk_import_expression(self, import);
    }
}

pub(super) fn discover(manifest: &mut SourceManifest, entry: &Path) -> Result<DependencyGraph, String> {
    let entry = entry.canonicalize().map_err(|error| error.to_string())?;
    let mut pending = BTreeSet::from([entry.clone()]);
    let mut graph = DependencyGraph::default();
    while let Some(path) = pending.pop_first() {
        if graph.files.contains_key(&path) { continue; }
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let extension = path.extension().and_then(|name| name.to_str());
        let mut file_kind = quench_runtime::SourceKind::Script;
        match extension {
            Some("json") => {},
            Some("node") => graph.requirements.push(GraphRequirement::NativeAddon { path: path.clone() }),
            Some("wasm") => { file_kind = quench_runtime::SourceKind::Module; graph.requirements.push(GraphRequirement::Wasm { path: path.clone() }); },
            _ => {
                let kind = commonjs::source_kind(&path)?;
                file_kind = kind;
                if kind == quench_runtime::SourceKind::Script {
                    manifest.insert_key(commonjs::native_commonjs_source(&path, &bytes, path == entry)?);
                } else {
                    graph.requirements.push(GraphRequirement::EsModule { path: path.clone() });
                }
                let text = commonjs::source_text(&bytes);
                let body = commonjs::script_body(&text);
                let base = u32::try_from(text.len() - body.len()).map_err(|_| "module source exceeds compiler positions")?;
                let allocator = oxc_allocator::Allocator::default();
                let source_type = oxc_span::SourceType::unambiguous()
                    .with_typescript(matches!(extension, Some("ts" | "cts" | "mts")));
                let parsed = oxc_parser::Parser::new(&allocator, body, source_type)
                    .with_options(oxc_parser::ParseOptions { allow_return_outside_function: true, ..Default::default() }).parse();
                if !parsed.diagnostics.is_empty() {
                    return Err(format!("{}: {:?}", path.display(), parsed.diagnostics));
                }
                let mut requests = Requests::default();
                requests.visit_program(&parsed.program);
                graph.host_globals.extend(std::mem::take(&mut requests.host_globals));
                requests.specifiers.extend(parsed.module_record.requested_modules.keys().map(ToString::to_string));
                for offset in requests.dynamic_requires {
                    graph.requirements.push(GraphRequirement::DynamicRequire { from: path.clone(), offset: offset + base });
                }
                for offset in requests.dynamic_imports {
                    graph.requirements.push(GraphRequirement::DynamicImport { from: path.clone(), offset: offset + base });
                }
                for specifier in requests.specifiers {
                    if commonjs::is_builtin_specifier(&specifier) {
                        graph.builtins.insert(specifier);
                        continue;
                    }
                    let directory = path.parent().ok_or("module has no parent directory")?;
                    let resolve = |resolver: &oxc_resolver::Resolver| resolver.resolve(directory, &specifier);
                    let resolved = if kind == quench_runtime::SourceKind::Script {
                        commonjs::with_commonjs_resolver(|resolver| commonjs::resolve_kept(resolver, resolve))
                    } else { commonjs::with_import_resolver(|resolver| commonjs::resolve_kept(resolver, resolve)) };
                    match resolved {
                        Ok(resolved) => {
                            let target = resolved.full_path().canonicalize().map_err(|error| error.to_string())?;
                            graph.resolutions.insert((path.clone(), specifier), target.clone());
                            pending.insert(target);
                        },
                        Err(error) => graph.requirements.push(GraphRequirement::Unresolved {
                            from: path.clone(), specifier, diagnostic: error.to_string(),
                        }),
                    }
                }
            },
        }
        graph.files.insert(path, ModuleFile { bytes: std::rc::Rc::from(bytes), kind: file_kind, prepared: None });
    }
    Ok(graph)
}
