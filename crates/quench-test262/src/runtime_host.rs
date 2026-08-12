//! Adapter from the runner contract to the residual runtime.

use std::{path::Path, sync::OnceLock};

use quench_runtime::module_bindings::ModuleBindingCell;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};
use quench_runtime::reduce::{
    inspect_module_source, reduce_module_sequence, reduce_module_source, reduce_script_sources,
    reduce_source, ScriptSource,
};
use quench_runtime::vm::{execute_with_context, ExecutionScope, VmContext};

use crate::module_graph::{ModuleGraph, ModuleId};
use crate::Test262Host;

#[derive(Debug, Default)]
pub struct RuntimeHost;

/// Independently reduced module unit with explicit live-cell linking.
pub struct LinkedModule {
    program: quench_runtime::reduce::ResidualProgram,
    scope: ExecutionScope,
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
            } else {
                LinkedModule::compile(&unit.source)?
            };
            units.insert(unit.id, module);
        }
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
                let cell = units
                    .get(&target)
                    .and_then(|unit| unit.export_cell(&binding.imported))
                    .ok_or_else(|| format!("export {} missing", binding.imported))?;
                units
                    .get(&id)
                    .ok_or_else(|| "module unit missing".to_string())?
                    .bind_import(&binding.local, cell)?;
            }
        }
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
}

impl LinkedModule {
    pub fn compile(source: &str) -> Result<Self, String> {
        let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
        Ok(Self {
            program,
            scope: ExecutionScope::new(),
        })
    }

    pub fn compile_with_prefix(prefix: &[&str], source: &str) -> Result<Self, String> {
        let program = reduce_module_sequence(prefix, source).map_err(|errors| errors.join("; "))?;
        Ok(Self {
            program,
            scope: ExecutionScope::new(),
        })
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
        if !self
            .program
            .module_metadata
            .as_ref()?
            .exported_names
            .iter()
            .any(|export| export == name)
        {
            return None;
        }
        self.program
            .local_slots
            .get(name)
            .copied()
            .map(|slot| self.scope.module_cell_slot(slot))
    }

    pub fn execute(&self) -> Result<quench_runtime::value::Value, String> {
        let mut registers = Vec::new();
        self.scope
            .execute(&self.program.ops, &mut registers, host_context())
            .map_err(|error| format!("residual VM error: {}", error.render()))
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
    let metadata = inspect_module_source(&source).map_err(|errors| errors.join("; "))?;
    for specifier in metadata.import_specifiers {
        if graph.resolve(from, &specifier).is_some() {
            continue;
        }
        let path = base.join(&specifier);
        let source = load_module_source(&path)?;
        let dependency = graph.add_dependency(path, source);
        load_module_dependencies(graph, dependency)?;
    }
    Ok(())
}

fn load_module_source(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("module {}: {error}", path.display()))?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        serde_json::from_str::<serde_json::Value>(&source)
            .map_err(|error| format!("SyntaxError: invalid JSON module: {error}"))?;
        return Ok(format!("export default {source};"));
    }
    Ok(source)
}

fn run_source(source: &str) -> Result<(), String> {
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program)
}

fn execute_program(program: &quench_runtime::reduce::ResidualProgram) -> Result<(), String> {
    quench_runtime::builtins::reset_intrinsic_prototype_state();
    execute_with_context(&program.ops, host_context())
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
