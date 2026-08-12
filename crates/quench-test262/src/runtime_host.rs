//! Adapter from the runner contract to the residual runtime.

use std::sync::OnceLock;

use quench_runtime::module_bindings::ModuleBindingCell;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};
use quench_runtime::reduce::{
    reduce_module_sequence, reduce_module_source, reduce_script_sources, reduce_source,
    ScriptSource,
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
        graph.link_all_units()?;
        let mut units = std::collections::HashMap::new();
        for unit in graph.units() {
            units.insert(unit.id, LinkedModule::compile(&unit.source)?);
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
