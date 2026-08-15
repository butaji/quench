//! Adapter from the runner contract to the residual runtime.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::OnceLock,
};

use quench_runtime::module_bindings::ModuleBindingCell;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};
use quench_runtime::reduce::{
    inspect_module_source, reduce_module_sequence, reduce_module_source, reduce_script_sources,
    reduce_source, ScriptSource,
};
use quench_runtime::vm::{execute_with_context, ExecutionScope, VmContext};

use crate::module_graph::{ModuleGraph, ModuleId, ModuleKind};
use crate::Test262Host;

#[derive(Debug, Default)]
pub struct RuntimeHost;

/// Independently reduced module unit with explicit live-cell linking.
pub struct LinkedModule {
    program: quench_runtime::reduce::ResidualProgram,
    scope: ExecutionScope,
    fixed_exports: Vec<(String, quench_runtime::value::Value)>,
    linked_exports: RefCell<HashMap<String, ModuleBindingCell>>,
    export_candidates: RefCell<HashMap<String, Vec<ModuleBindingCell>>>,
    ambiguous_exports: RefCell<HashSet<String>>,
    namespace: RefCell<Option<ModuleBindingCell>>,
}

/// Graph-owned collection of independently compiled linked modules.
pub struct LinkedModuleGraph {
    units: std::collections::HashMap<ModuleId, LinkedModule>,
}

impl LinkedModuleGraph {
    pub fn compile(graph: &mut ModuleGraph) -> Result<Self, String> {
        Self::compile_with_entry_prefix(graph, None, &[])
    }

    pub fn compile_with_entry_prefix(
        graph: &mut ModuleGraph,
        entry: Option<ModuleId>,
        prefix: &[&str],
    ) -> Result<Self, String> {
        graph.link_all_units()?;
        let mut units = std::collections::HashMap::new();
        for unit in graph.units() {
            let module = if Some(unit.id) == entry {
                LinkedModule::compile_with_prefix(prefix, &unit.source)?
            } else if unit.kind == ModuleKind::Json {
                LinkedModule::compile_json(&unit.source)?
            } else if unit.kind == ModuleKind::Text {
                LinkedModule::compile_text(&unit.source)?
            } else {
                LinkedModule::compile(&unit.source)?
            };
            units.insert(unit.id, module);
        }
        link_reexports(graph, &units)?;
        bind_imports(graph, &units, true)?;
        for unit in units.values() {
            unit.reset_links();
        }
        link_reexports(graph, &units)?;
        bind_imports(graph, &units, true)?;
        for unit in units.values() {
            unit.reset_links();
        }
        link_reexports(graph, &units)?;
        bind_imports(graph, &units, false)?;
        Ok(Self { units })
    }

    pub fn execute(&self, graph: &ModuleGraph, entry: ModuleId) -> Result<(), String> {
        for id in graph.dependency_order(entry)? {
            self.units
                .get(&id)
                .ok_or_else(|| "module unit missing".to_string())?
                .execute()?;
        }
        Ok(())
    }

    pub fn export_cell(&self, unit: ModuleId, name: &str) -> Option<ModuleBindingCell> {
        self.units.get(&unit)?.export_cell(name)
    }
}

fn bind_imports(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
    provisional: bool,
) -> Result<(), String> {
    let ids = units.keys().copied().collect::<Vec<_>>();
    for id in ids {
        let metadata = units
            .get(&id)
            .and_then(|unit| unit.program.module_metadata.as_ref())
            .ok_or_else(|| "module metadata missing".to_string())?;
        for binding in &metadata.imports {
            let target = graph
                .resolve(id, &binding.source)
                .ok_or_else(|| format!("unresolved module {}", binding.source))?;
            let cell = import_cell(units, target, &binding.imported, provisional)?;
            units
                .get(&id)
                .ok_or_else(|| "module unit missing".to_string())?
                .bind_import(&binding.local, cell)?;
        }
    }
    Ok(())
}

fn link_reexports(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
) -> Result<(), String> {
    for unit in graph.units() {
        let metadata = unit_metadata(units, unit.id)?;
        for binding in &metadata.reexports {
            let target = resolve_reexport(graph, unit.id, &binding.source)?;
            link_reexport(units, unit.id, target, binding)?;
        }
    }
    Ok(())
}

fn unit_metadata(
    units: &HashMap<ModuleId, LinkedModule>,
    id: ModuleId,
) -> Result<&quench_runtime::reduce::ModuleMetadata, String> {
    units
        .get(&id)
        .and_then(|unit| unit.program.module_metadata.as_ref())
        .ok_or_else(|| "module metadata missing".to_string())
}

fn resolve_reexport(graph: &ModuleGraph, from: ModuleId, source: &str) -> Result<ModuleId, String> {
    graph
        .resolve(from, source)
        .ok_or_else(|| format!("unresolved module {source}"))
}

fn link_reexport(
    units: &HashMap<ModuleId, LinkedModule>,
    from: ModuleId,
    target: ModuleId,
    binding: &quench_runtime::reduce::ReexportBinding,
) -> Result<(), String> {
    if binding.imported == "*all*" {
        return link_star_exports(units, from, target);
    }
    let cell = import_cell(units, target, &binding.imported, true)?;
    units
        .get(&from)
        .ok_or_else(|| "module unit missing".to_string())?
        .link_export(&binding.exported, cell);
    Ok(())
}

fn link_star_exports(
    units: &HashMap<ModuleId, LinkedModule>,
    from: ModuleId,
    target: ModuleId,
) -> Result<(), String> {
    let names = unit_metadata(units, target)?.exported_names.clone();
    let from_unit = units
        .get(&from)
        .ok_or_else(|| "module unit missing".to_string())?;
    for name in names.into_iter().filter(|name| name != "default") {
        if let Some(cell) = units.get(&target).and_then(|unit| unit.export_cell(&name)) {
            from_unit.link_export(&name, cell);
        }
    }
    Ok(())
}

fn import_cell(
    units: &std::collections::HashMap<ModuleId, LinkedModule>,
    target: ModuleId,
    imported: &str,
    provisional: bool,
) -> Result<ModuleBindingCell, String> {
    if imported == "*" {
        return namespace_cell(units, target);
    }
    units
        .get(&target)
        .and_then(|unit| {
            if provisional {
                unit.provisional_export_cell(imported)
            } else {
                unit.export_cell(imported)
            }
        })
        .ok_or_else(|| format!("SyntaxError: export {imported} missing"))
}

fn namespace_cell(
    units: &std::collections::HashMap<ModuleId, LinkedModule>,
    target: ModuleId,
) -> Result<ModuleBindingCell, String> {
    let unit = units
        .get(&target)
        .ok_or_else(|| "module unit missing".to_string())?;
    if let Some(namespace) = unit.namespace.borrow().as_ref() {
        return Ok(namespace.clone());
    }
    let properties = unit
        .export_names()
        .iter()
        .filter_map(|name| unit.export_cell(name).map(|cell| (name.clone(), cell)))
        .map(|(name, cell)| {
            (
                name,
                quench_runtime::value::Value::BindingCell(cell.shared()),
            )
        })
        .collect();
    let namespace = ModuleBindingCell::new(quench_runtime::value::Value::object(properties));
    unit.namespace.replace(Some(namespace.clone()));
    Ok(namespace)
}

impl LinkedModule {
    pub fn compile(source: &str) -> Result<Self, String> {
        let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
        Ok(Self {
            program,
            scope: ExecutionScope::new(),
            fixed_exports: Vec::new(),
            linked_exports: RefCell::new(HashMap::new()),
            export_candidates: RefCell::new(HashMap::new()),
            ambiguous_exports: RefCell::new(HashSet::new()),
            namespace: RefCell::new(None),
        })
    }

    pub fn compile_with_prefix(prefix: &[&str], source: &str) -> Result<Self, String> {
        let program = reduce_module_sequence(prefix, source).map_err(|errors| errors.join("; "))?;
        Ok(Self {
            program,
            scope: ExecutionScope::new(),
            fixed_exports: Vec::new(),
            linked_exports: RefCell::new(HashMap::new()),
            export_candidates: RefCell::new(HashMap::new()),
            ambiguous_exports: RefCell::new(HashSet::new()),
            namespace: RefCell::new(None),
        })
    }

    pub fn compile_json(source: &str) -> Result<Self, String> {
        let value = quench_runtime::parse_json(source)
            .map_err(|error| format!("SyntaxError: invalid JSON module: {error}"))?;
        let mut module = Self::compile("export default null;")?;
        module.fixed_exports.push(("default".to_string(), value));
        Ok(module)
    }

    pub fn compile_text(source: &str) -> Result<Self, String> {
        let literal = serde_json::to_string(source).map_err(|error| error.to_string())?;
        Self::compile(&format!("export default {literal};"))
    }

    pub fn bind_import(&self, local: &str, cell: ModuleBindingCell) -> Result<(), String> {
        let slot = self
            .program
            .local_slots
            .get(local)
            .copied()
            .ok_or_else(|| format!("unknown module import binding {local}"))?;
        self.scope.bind_module_slot(slot, cell);
        Ok(())
    }

    pub fn export_cell(&self, name: &str) -> Option<ModuleBindingCell> {
        if self.ambiguous_exports.borrow().contains(name) {
            return None;
        }
        if let Some(cell) = self.linked_exports.borrow().get(name) {
            return Some(cell.clone());
        }
        if self.ambiguous_exports.borrow().contains(name) {
            return None;
        }
        let local = self
            .program
            .module_metadata
            .as_ref()?
            .exports
            .iter()
            .find(|binding| binding.exported == name)?
            .local
            .as_str();
        self.program
            .local_slots
            .get(local)
            .copied()
            .map(|slot| self.scope.module_cell_slot(slot))
    }

    fn provisional_export_cell(&self, name: &str) -> Option<ModuleBindingCell> {
        if let Some(cell) = self.linked_exports.borrow().get(name) {
            return Some(cell.clone());
        }
        if let Some(cell) = self
            .export_candidates
            .borrow()
            .get(name)
            .and_then(|candidates| candidates.first())
        {
            return Some(cell.clone());
        }
        self.export_cell(name)
    }

    fn export_names(&self) -> Vec<String> {
        let mut names = self
            .program
            .module_metadata
            .as_ref()
            .map_or_else(Vec::new, |metadata| metadata.exported_names.clone());
        for name in self.linked_exports.borrow().keys() {
            if !self.ambiguous_exports.borrow().contains(name) && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    fn link_export(&self, name: &str, cell: ModuleBindingCell) {
        let mut candidates = self.export_candidates.borrow_mut();
        let entries = candidates.entry(name.to_string()).or_default();
        if entries
            .iter()
            .any(|candidate| Rc::ptr_eq(&candidate.shared(), &cell.shared()))
        {
            return;
        }
        entries.push(cell.clone());
        let unique = entries.len();
        drop(candidates);
        if unique == 1 {
            self.linked_exports
                .borrow_mut()
                .insert(name.to_string(), cell);
        } else {
            self.linked_exports.borrow_mut().remove(name);
            self.ambiguous_exports.borrow_mut().insert(name.to_string());
        }
    }

    fn reset_links(&self) {
        self.linked_exports.borrow_mut().clear();
        self.export_candidates.borrow_mut().clear();
        self.ambiguous_exports.borrow_mut().clear();
    }

    pub fn execute(&self) -> Result<quench_runtime::value::Value, String> {
        let mut registers = Vec::new();
        let result = self
            .scope
            .execute(self.program.ops(), &mut registers, host_context())
            .map_err(|error| format!("residual VM error: {}", error.render()))?;
        for (name, value) in &self.fixed_exports {
            let cell = self
                .export_cell(name)
                .ok_or_else(|| format!("fixed export {name} missing"))?;
            cell.set(value.clone());
        }
        Ok(result)
    }
}

impl Test262Host for RuntimeHost {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        run_source(source)
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        run_module_source(source)
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String> {
        let mut scripts = harness
            .iter()
            .map(|source| ScriptSource {
                source,
                strict: false,
            })
            .collect::<Vec<_>>();
        scripts.push(ScriptSource { source, strict });
        let program = reduce_script_sources(&scripts).map_err(|errors| errors.join("; "))?;
        execute_program(&program)
    }

    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String> {
        let program =
            reduce_module_sequence(harness, source).map_err(|errors| errors.join("; "))?;
        execute_program(&program)
    }

    fn run_harnessed_module_at(
        &mut self,
        harness: &[&str],
        source: &str,
        path: &Path,
    ) -> Result<(), String> {
        reduce_module_sequence(harness, source).map_err(|errors| errors.join("; "))?;
        let mut graph = module_graph(path, source)?;
        let entry = graph
            .entry()
            .ok_or_else(|| "module graph missing entry".to_string())?;
        let linked =
            LinkedModuleGraph::compile_with_entry_prefix(&mut graph, Some(entry), harness)?;
        quench_runtime::builtins::reset_intrinsic_prototype_state();
        linked.execute(&graph, entry)
    }
}

fn module_graph(path: &Path, source: &str) -> Result<ModuleGraph, String> {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(path.to_path_buf(), source.to_string());
    load_module_dependencies(&mut graph, entry)?;
    Ok(graph)
}

fn load_module_dependencies(graph: &mut ModuleGraph, from: ModuleId) -> Result<(), String> {
    let (base, source) = graph
        .unit(from)
        .map(|unit| {
            (
                unit.path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf(),
                unit.source.clone(),
            )
        })
        .ok_or_else(|| "module unit is unknown".to_string())?;
    if graph
        .unit(from)
        .is_some_and(|unit| unit.kind != ModuleKind::JavaScript)
    {
        return Ok(());
    }
    let metadata = inspect_module_source(&source).map_err(|errors| errors.join("; "))?;
    for specifier in metadata.import_specifiers {
        if graph.resolve(from, &specifier).is_some() {
            continue;
        }
        let path = base.join(&specifier);
        let dependency = add_module_source(graph, path, &metadata.import_attributes, &specifier)?;
        load_module_dependencies(graph, dependency)?;
    }
    Ok(())
}

fn add_module_source(
    graph: &mut ModuleGraph,
    path: std::path::PathBuf,
    attributes: &[(String, String)],
    specifier: &str,
) -> Result<ModuleId, String> {
    let resource_type = attributes
        .iter()
        .find(|(source, _)| source == specifier)
        .map(|(_, attribute)| attribute.as_str());
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("module {}: {error}", path.display()))?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return Ok(graph.add_json_dependency(path, source));
    }
    if resource_type == Some("type=text") {
        return Ok(graph.add_text_dependency(path, source));
    }
    Ok(graph.add_dependency(path, source))
}

fn run_source(source: &str) -> Result<(), String> {
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program)
}

fn execute_program(program: &quench_runtime::reduce::ResidualProgram) -> Result<(), String> {
    quench_runtime::builtins::reset_intrinsic_prototype_state();
    execute_with_context(program.ops(), host_context())
        .map(|_| ())
        .map_err(|error| format!("residual VM error: {}", error.render()))
}

fn run_module_source(source: &str) -> Result<(), String> {
    let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program)
}

fn host_context() -> &'static VmContext {
    static CONTEXT: OnceLock<VmContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        VmContext::for_realm(
            RealmId::ROOT,
            vec![
                HostCapabilityKind::GetGlobal,
                HostCapabilityKind::CreateRealm,
                HostCapabilityKind::EvalScript,
                HostCapabilityKind::DetachArrayBuffer,
            ],
        )
        .with_host_capability(
            "$262",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::GetGlobal,
            },
        )
    })
}

#[cfg(test)]
mod tests {
    include!("runtime_host_tests.rs");
}
