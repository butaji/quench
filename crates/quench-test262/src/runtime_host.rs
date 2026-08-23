//! Adapter from the runner contract to the residual runtime.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
};

use quench_runtime::module_bindings::ModuleBindingCell;
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
    star_exports: RefCell<HashSet<String>>,
    ambiguous_exports: RefCell<HashSet<String>>,
    namespace_cell: RefCell<Option<ModuleBindingCell>>,
    deferred_namespace_cell: RefCell<Option<ModuleBindingCell>>,
    module_source: ModuleBindingCell,
    evaluated: Cell<bool>,
    started: Cell<bool>,
    evaluating: Cell<bool>,
    thrown: RefCell<Option<quench_runtime::value::Value>>,
    resume_pc: Cell<usize>,
    resume_registers: RefCell<Vec<quench_runtime::value::Value>>,
    async_suspended: Cell<bool>,
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
            } else if unit.kind == ModuleKind::Bytes {
                LinkedModule::compile_bytes(&unit.bytes)?
            } else {
                LinkedModule::compile(&unit.source)?
            };
            units.insert(unit.id, module);
        }
        let root = entry
            .or_else(|| graph.entry())
            .ok_or_else(|| "module graph missing entry".to_string())?;
        let order = graph.dependency_order(root)?;
        bind_imports(&units, graph, &order, |binding| {
            binding.imported == "source"
        })?;
        for _ in 0..units.len() {
            for id in &order {
                link_reexports(graph, &units, *id)?;
            }
        }
        bind_imports(&units, graph, &order, |binding| {
            binding.imported != "source"
        })?;
        Ok(Self { units })
    }

    pub fn execute(&self, graph: &ModuleGraph, entry: ModuleId) -> Result<(), String> {
        let _shared = quench_runtime::vm::SharedGlobal::install();
        quench_runtime::module_bindings::reset_module_jobs();
        quench_runtime::module_bindings::defer_fulfilled_await(true);
        let graph_ptr = self as *const LinkedModuleGraph;
        let modules_ptr = graph as *const ModuleGraph;
        CURRENT_MODULE_GRAPH.with(|current| {
            current.set(Some((graph_ptr, modules_ptr)));
        });
        let _import = quench_runtime::module_bindings::install_dynamic_import(Rc::new(
            move |specifier, deferred| {
                CURRENT_MODULE_ID.with(|id| {
                    let from = id.get().or_else(|| unsafe { &*modules_ptr }.entry())?;
                    let target = unsafe { &*modules_ptr }.resolve(from, specifier)?;
                    if unsafe { &*modules_ptr }.has_deferred_resolution_error(target) {
                        quench_runtime::module_bindings::request_ensure_type_error();
                        return None;
                    }
                    if !deferred {
                        let _ = evaluate_module(
                            unsafe { &*graph_ptr },
                            unsafe { &*modules_ptr },
                            target,
                            true,
                        );
                        settle_dynamic_import(unsafe { &*graph_ptr }, target);
                    }
                    import_cell(
                        unsafe { &*modules_ptr },
                        &unsafe { &*graph_ptr }.units,
                        target,
                        "*",
                        deferred,
                    )
                    .ok()
                    .map(|cell| cell.get())
                })
            },
        ));
        let result = evaluate_module(self, graph, entry, true);
        quench_runtime::module_bindings::drain_jobs();
        let result = match self
            .units
            .get(&entry)
            .and_then(|unit| unit.thrown.borrow().clone())
        {
            Some(thrown) => {
                quench_runtime::module_bindings::request_ensure_throw(thrown.clone());
                Err(format!(
                    "residual VM error: {}",
                    quench_runtime::execute::VmError::Thrown(thrown).render()
                ))
            }
            None => result,
        };
        CURRENT_MODULE_GRAPH.with(|current| current.set(None));
        quench_runtime::module_bindings::defer_fulfilled_await(false);
        result
    }

    pub fn export_cell(&self, unit: ModuleId, name: &str) -> Option<ModuleBindingCell> {
        self.units.get(&unit)?.export_cell(name)
    }
}

include!("runtime_host_linking.rs");

impl LinkedModule {
    pub fn compile(source: &str) -> Result<Self, String> {
        let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
        Ok(Self {
            program,
            scope: ExecutionScope::new(),
            fixed_exports: Vec::new(),
            linked_exports: RefCell::new(HashMap::new()),
            star_exports: RefCell::new(HashSet::new()),
            ambiguous_exports: RefCell::new(HashSet::new()),
            namespace_cell: RefCell::new(None),
            deferred_namespace_cell: RefCell::new(None),
            module_source: module_source_cell(),
            evaluated: Cell::new(false),
            started: Cell::new(false),
            evaluating: Cell::new(false),
            thrown: RefCell::new(None),
            resume_pc: Cell::new(0),
            resume_registers: RefCell::new(Vec::new()),
            async_suspended: Cell::new(false),
        })
    }

    pub fn compile_with_prefix(prefix: &[&str], source: &str) -> Result<Self, String> {
        let program = reduce_module_sequence(prefix, source).map_err(|errors| errors.join("; "))?;
        Ok(Self {
            program,
            scope: ExecutionScope::new(),
            fixed_exports: Vec::new(),
            linked_exports: RefCell::new(HashMap::new()),
            star_exports: RefCell::new(HashSet::new()),
            ambiguous_exports: RefCell::new(HashSet::new()),
            namespace_cell: RefCell::new(None),
            deferred_namespace_cell: RefCell::new(None),
            module_source: module_source_cell(),
            evaluated: Cell::new(false),
            started: Cell::new(false),
            evaluating: Cell::new(false),
            thrown: RefCell::new(None),
            resume_pc: Cell::new(0),
            resume_registers: RefCell::new(Vec::new()),
            async_suspended: Cell::new(false),
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
        let mut module = Self::compile("export default null;")?;
        module.fixed_exports.push((
            "default".to_string(),
            quench_runtime::value::Value::String(source.to_string()),
        ));
        Ok(module)
    }

    pub fn compile_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut module = Self::compile("export default null;")?;
        let buffer = quench_runtime::value::ArrayBufferData::new(0);
        *buffer.bytes.borrow_mut() = bytes.to_vec();
        let buffer = std::rc::Rc::new(buffer.transfer_to_immutable());
        let view = quench_runtime::value::Uint8ArrayData::new(buffer, 0, bytes.len());
        module.fixed_exports.push((
            "default".to_string(),
            quench_runtime::value::Value::Uint8Array(std::rc::Rc::new(view)),
        ));
        Ok(module)
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
        let binding = self
            .program
            .module_metadata
            .as_ref()?
            .exports
            .iter()
            .find(|binding| binding.exported == name)?;
        let local = binding.local.as_str();
        self.program
            .local_slots
            .get(local)
            .copied()
            .map(|slot| self.scope.module_cell_slot(slot))
    }

    fn export_names(&self) -> Vec<String> {
        let mut names = self
            .program
            .module_metadata
            .as_ref()
            .map_or_else(Vec::new, |metadata| metadata.exported_names.clone());
        names.retain(|name| !self.ambiguous_exports.borrow().contains(name));
        for name in self.linked_exports.borrow().keys() {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    fn link_export(&self, name: &str, cell: ModuleBindingCell) {
        self.linked_exports
            .borrow_mut()
            .insert(name.to_string(), cell);
        self.refresh_namespace();
    }

    fn link_star_export(&self, name: &str, cell: ModuleBindingCell) {
        self.link_export(name, cell);
        self.star_exports.borrow_mut().insert(name.to_string());
    }

    fn mark_ambiguous_export(&self, name: &str) {
        self.linked_exports.borrow_mut().remove(name);
        self.star_exports.borrow_mut().remove(name);
        self.ambiguous_exports.borrow_mut().insert(name.to_string());
    }

    fn has_star_export(&self, name: &str) -> bool {
        self.star_exports.borrow().contains(name)
    }

    fn same_star_export(&self, name: &str, cell: &ModuleBindingCell) -> bool {
        self.linked_exports
            .borrow()
            .get(name)
            .is_some_and(|existing| Rc::ptr_eq(&existing.shared(), &cell.shared()))
    }

    fn has_local_export(&self, name: &str) -> bool {
        self.program
            .module_metadata
            .as_ref()
            .is_some_and(|metadata| {
                metadata
                    .exports
                    .iter()
                    .any(|binding| binding.exported == name)
            })
    }

    fn is_ambiguous_export(&self, name: &str) -> bool {
        self.ambiguous_exports.borrow().contains(name)
    }
}

thread_local! {
    static CURRENT_MODULE_GRAPH: Cell<Option<(*const LinkedModuleGraph, *const ModuleGraph)>> =
        const { Cell::new(None) };
    static CURRENT_MODULE_ID: Cell<Option<ModuleId>> = const { Cell::new(None) };
}

include!("runtime_host_namespace.rs");
include!("runtime_host_execute.rs");
include!("runtime_host_eval.rs");

fn module_source_cell() -> ModuleBindingCell {
    ModuleBindingCell::new(quench_runtime::value::Value::object(vec![(
        "\0prototype".to_string(),
        quench_runtime::value::Value::Builtin(
            quench_runtime::ops::Builtin::AbstractModuleSourcePrototype,
        ),
    )]))
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
        // AGENTS.md: harness fidelity is absolute; dispatch exact sources without rewriting.
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
        // AGENTS.md: harness fidelity is absolute; compose and dispatch exact harness sources.
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
        // AGENTS.md: harness fidelity is absolute; compose and dispatch exact harness sources.
        reduce_module_sequence(harness, source).map_err(|errors| errors.join("; "))?;
        let mut graph = module_graph(path, source)?;
        let entry = graph
            .entry()
            .ok_or_else(|| "module graph missing entry".to_string())?;
        let linked =
            LinkedModuleGraph::compile_with_entry_prefix(&mut graph, Some(entry), harness)?;
        quench_runtime::builtins::reset_intrinsic_prototype_state();
        quench_runtime::execute::reset_replacements();
        linked.execute(&graph, entry)
    }
}

include!("runtime_host_graph.rs");

fn run_source(source: &str) -> Result<(), String> {
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program)
}

fn execute_program(program: &quench_runtime::reduce::ResidualProgram) -> Result<(), String> {
    quench_runtime::builtins::reset_intrinsic_prototype_state();
    quench_runtime::execute::reset_replacements();
    execute_with_context(program.ops(), host_context())
        .map(|_| ())
        .map_err(|error| format!("residual VM error: {}", error.render()))
}

fn host_context() -> &'static VmContext {
    thread_local! {
        static CONTEXT: std::cell::OnceCell<&'static VmContext> = const { std::cell::OnceCell::new() };
    }
    CONTEXT.with(|context| {
        *context.get_or_init(|| {
            Box::leak(Box::new(
                VmContext::for_realm(
                    quench_runtime::ops::RealmId::ROOT,
                    vec![
                        quench_runtime::ops::HostCapabilityKind::GetGlobal,
                        quench_runtime::ops::HostCapabilityKind::CreateRealm,
                        quench_runtime::ops::HostCapabilityKind::EvalScript,
                        quench_runtime::ops::HostCapabilityKind::DetachArrayBuffer,
                        quench_runtime::ops::HostCapabilityKind::IsHTMLDDA,
                    ],
                )
                .with_host_capability(
                    "$262",
                    quench_runtime::ops::HostCapabilityRef {
                        realm: quench_runtime::ops::RealmId::ROOT,
                        kind: quench_runtime::ops::HostCapabilityKind::GetGlobal,
                    },
                ),
            ))
        })
    })
}

fn run_module_source(source: &str) -> Result<(), String> {
    let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program)
}

#[cfg(test)]
mod tests {
    include!("runtime_host_tests.rs");
    include!("runtime_host_determinism_tests.rs");
}
