//! Adapter from the runner contract to the residual runtime.

use std::{
    cell::{Cell, RefCell},
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
use quench_runtime::value::Value;
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
    executed: Cell<bool>,
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
                LinkedModule::compile_bytes(unit.bytes.as_deref().unwrap_or_default())?
            } else {
                LinkedModule::compile(&unit.source)?
            };
            units.insert(unit.id, module);
        }
        bind_namespace_imports(graph, &units)?;
        for unit in units.values() {
            unit.reset_links();
        }
        link_reexports(graph, &units)?;
        link_reexports(graph, &units)?;
        link_reexports(graph, &units)?;
        bind_imports(graph, &units, false)?;
        for unit in units.values() {
            for name in unit.export_names() {
                unit.refresh_namespace_export(&name);
            }
        }
        Ok(Self { units })
    }

    pub fn execute(&self, graph: &ModuleGraph, entry: ModuleId) -> Result<(), String> {
        let graph_ptr = self as *const Self;
        let module_graph_ptr = graph as *const ModuleGraph;
        let _callback = quench_runtime::vm::install_deferred_module_callback(Rc::new(move |id| {
            // The callback guard lives only for the duration of this execution.
            // Both pointed-to graphs are borrowed by the caller for that duration.
            let target = ModuleId(id);
            let unit = unsafe { (&*graph_ptr).units.get(&target) };
            if unit.is_some_and(LinkedModule::is_evaluating) {
                return Err(quench_runtime::execute::VmError::Thrown(
                    quench_runtime::builtins::error(
                        quench_runtime::ops::Builtin::TypeError,
                        &[Value::String(
                            "Cannot evaluate a module while it is evaluating".to_string(),
                        )],
                    ),
                ));
            }
            if let Some(unit) = unit {
                if let Some(reason) = unit.error_value() {
                    unit.restore_deferred_markers(target.0);
                    return Err(quench_runtime::execute::VmError::Thrown(reason));
                }
            }
            match unsafe { (&*graph_ptr).execute(&*module_graph_ptr, target) } {
                Ok(()) => {
                    if let Some(unit) = unsafe { (&*graph_ptr).units.get(&target) }
                        .filter(|unit| unit.is_executed())
                    {
                        unit.complete_deferred_namespace();
                        unit.clear_deferred_markers();
                    }
                    Ok(Value::Undefined)
                }
                Err(error) => {
                    let Some(unit) = (unsafe { (&*graph_ptr).units.get(&target) }) else {
                        return Err(quench_runtime::execute::VmError::EvalError(error));
                    };
                    unit.restore_deferred_markers(target.0);
                    match unit.error_value() {
                        Some(reason) => Err(quench_runtime::execute::VmError::Thrown(reason)),
                        None => Err(quench_runtime::execute::VmError::EvalError(error)),
                    }
                }
            }
        }));
        let graph_ptr = self as *const Self;
        let module_graph_ptr = graph as *const ModuleGraph;
        let _dynamic = quench_runtime::vm::install_dynamic_import_callback(Rc::new(
            move |specifier, deferred, options| {
                let graph = unsafe { &*module_graph_ptr };
                let kind = dynamic_module_kind(&options);
                let Some(target) = graph
                    .resolve_kind(entry, &specifier, kind)
                    .or_else(|| graph.resolve_kind(entry, &specifier, ModuleKind::Json))
                    .or_else(|| graph.resolve_kind(entry, &specifier, ModuleKind::Text))
                    .or_else(|| graph.resolve_kind(entry, &specifier, ModuleKind::JavaScript))
                else {
                    let reason = dynamic_import_error("Cannot resolve dynamic import");
                    return Ok(Value::Promise(Rc::new(
                        quench_runtime::value::PromiseData::new(
                            quench_runtime::value::PromiseState::Rejected(reason),
                        ),
                    )));
                };
                let namespace = namespace_cell(unsafe { &(*graph_ptr).units }, target, deferred)
                    .map_err(quench_runtime::execute::VmError::EvalError)?;
                if !deferred {
                    if let Err(error) = unsafe { (&*graph_ptr).execute(graph, target) } {
                        let reason = dynamic_import_error(&error);
                        return Ok(Value::Promise(Rc::new(
                            quench_runtime::value::PromiseData::new(
                                quench_runtime::value::PromiseState::Rejected(reason),
                            ),
                        )));
                    }
                }
                Ok(fulfilled_import(namespace.get()))
            },
        ));
        let order = graph.dependency_order(entry)?;
        for id in &order {
            self.units
                .get(id)
                .ok_or_else(|| "module unit missing".to_string())?
                .instantiate()?;
        }
        let mut completed = HashSet::new();
        while completed.len() < order.len() {
            let mut progressed = false;
            for id in &order {
                if completed.contains(id) || !self.ready_for_execution(graph, *id) {
                    continue;
                }
                let unit = self
                    .units
                    .get(id)
                    .ok_or_else(|| "module unit missing".to_string())?;
                unit.execute()?;
                completed.insert(*id);
                progressed = true;
            }
            self.resume_async_modules_once(&order)?;
            if !progressed && !self.has_pending_async(&order) {
                for id in &order {
                    if completed.insert(*id) {
                        self.units
                            .get(id)
                            .ok_or_else(|| "module unit missing".to_string())?
                            .execute()?;
                    }
                }
            }
        }
        while self.has_pending_async(&order) {
            self.resume_async_modules_once(&order)?;
        }
        Ok(())
    }

    fn ready_for_execution(&self, graph: &ModuleGraph, id: ModuleId) -> bool {
        let mut seen = HashSet::new();
        self.dependencies_settled(graph, id, &mut seen)
    }

    fn dependencies_settled(
        &self,
        graph: &ModuleGraph,
        id: ModuleId,
        seen: &mut HashSet<ModuleId>,
    ) -> bool {
        if !seen.insert(id) {
            return true;
        }
        graph.dependencies(id).into_iter().all(|dependency| {
            self.units
                .get(&dependency)
                .map_or(true, |unit| unit.async_next.get().is_none())
                && self.dependencies_settled(graph, dependency, seen)
        })
    }

    fn resume_async_modules_once(&self, order: &[ModuleId]) -> Result<(), String> {
        for id in order {
            if let Some(unit) = self.units.get(id) {
                if unit.async_next.get().is_some() {
                    unit.resume_async()?;
                }
            }
        }
        Ok(())
    }

    fn has_pending_async(&self, order: &[ModuleId]) -> bool {
        order.iter().any(|id| {
            self.units
                .get(id)
                .is_some_and(|unit| unit.async_next.get().is_some())
        })
    }

    pub fn export_cell(&self, unit: ModuleId, name: &str) -> Option<ModuleBindingCell> {
        self.units.get(&unit)?.export_cell(name)
    }
}

fn bind_namespace_imports(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
) -> Result<(), String> {
    for id in units.keys().copied().collect::<Vec<_>>() {
        let metadata = unit_metadata(units, id)?;
        for binding in metadata
            .imports
            .iter()
            .filter(|binding| binding.imported == "*")
        {
            let target = graph
                .resolve(id, &binding.source)
                .ok_or_else(|| format!("unresolved module {}", binding.source))?;
            let cell = namespace_cell(units, target)?;
            units
                .get(&id)
                .ok_or_else(|| "module unit missing".to_string())?
                .bind_import(&binding.local, cell)?;
        }
    }
    Ok(())
}

fn bind_imports(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
    provisional: bool,
) -> Result<(), String> {
    for id in units.keys().copied().collect::<Vec<_>>() {
        let metadata = unit_metadata(units, id)?;
        for source_phase in [true, false] {
            for binding in metadata
                .imports
                .iter()
                .filter(|binding| binding.source_phase == source_phase)
            {
                let cell = if binding.source_phase {
                    ModuleBindingCell::new(quench_runtime::value::Value::object(vec![(
                        "\0quench:module-source".to_string(),
                        quench_runtime::value::Value::Boolean(true),
                    )]))
                } else {
                    let target = graph
                        .resolve(id, &binding.source)
                        .ok_or_else(|| format!("unresolved module {}", binding.source))?;
                    import_cell(units, target, &binding.imported, provisional)?
                };
                units
                    .get(&id)
                    .ok_or_else(|| "module unit missing".to_string())?
                    .bind_import(&binding.local, cell)?;
            }
        }
    }
    Ok(())
}

fn link_reexports(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
) -> Result<(), String> {
    let order = graph
        .entry()
        .and_then(|entry| graph.dependency_order(entry).ok())
        .unwrap_or_else(|| graph.units().iter().map(|unit| unit.id).collect());
    for id in order {
        let metadata = unit_metadata(units, id)?;
        for binding in &metadata.reexports {
            let target = resolve_reexport(graph, id, &binding.source)?;
            link_reexport(graph, units, id, target, binding)?;
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
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
    from: ModuleId,
    target: ModuleId,
    binding: &quench_runtime::reduce::ReexportBinding,
) -> Result<(), String> {
    if binding.imported == "*all*" {
        return link_star_exports(units, from, target);
    }
    if binding.imported == "*" {
        let cell = namespace_cell(units, target)?;
        units
            .get(&from)
            .ok_or_else(|| "module unit missing".to_string())?
            .link_export(&binding.exported, cell);
        return Ok(());
    }
    let cell = resolved_export_cell(units, graph, target, &binding.imported, &mut Vec::new())
        .ok_or_else(|| format!("SyntaxError: export {} missing", binding.imported))?;
    units
        .get(&from)
        .ok_or_else(|| "module unit missing".to_string())?
        .link_export(&binding.exported, cell);
    Ok(())
}

fn resolved_export_cell(
    units: &HashMap<ModuleId, LinkedModule>,
    graph: &ModuleGraph,
    id: ModuleId,
    name: &str,
    seen: &mut Vec<(ModuleId, String)>,
) -> Option<ModuleBindingCell> {
    let key = (id, name.to_string());
    if seen.contains(&key) {
        return None;
    }
    seen.push(key);
    if let Some(cell) = units.get(&id)?.export_cell(name) {
        return Some(cell);
    }
    let metadata = units.get(&id)?.program.module_metadata.as_ref()?;
    if let Some(binding) = metadata
        .reexports
        .iter()
        .find(|binding| binding.exported == name && binding.imported != "*all*")
    {
        let target = graph.resolve(id, &binding.source)?;
        return resolved_export_cell(units, graph, target, &binding.imported, seen);
    }
    let matches = metadata
        .reexports
        .iter()
        .filter(|binding| binding.imported == "*all*")
        .filter_map(|binding| {
            let target = graph.resolve(id, &binding.source)?;
            resolved_export_cell(units, graph, target, name, &mut seen.clone())
        })
        .collect::<Vec<_>>();
    let first = matches.first()?.clone();
    matches
        .iter()
        .all(|candidate| Rc::ptr_eq(&candidate.shared(), &first.shared()))
        .then_some(first)
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
        return namespace_cell(units, target, deferred);
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
    deferred: bool,
) -> Result<ModuleBindingCell, String> {
    let unit = units
        .get(&target)
        .ok_or_else(|| "module unit missing".to_string())?;
    if let Some(namespace) = unit.namespace.borrow().as_ref() {
        return Ok(namespace.clone());
    }
    let mut properties = vec![(
        "\0prototype".to_string(),
        quench_runtime::value::Value::Null,
    )];
    let mut export_names = unit.export_names();
    export_names.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
    properties.extend(
        export_names
            .iter()
            .filter_map(|name| unit.export_cell(name).map(|cell| (name.clone(), cell)))
            .map(|(name, cell)| {
                (
                    name,
                    quench_runtime::value::Value::BindingCell(cell.shared()),
                )
            })
            .collect::<Vec<_>>(),
    );
    for name in export_names {
        properties.push((
            format!("\0quench:descriptor:\0{name}"),
            descriptor_value(unit, &name),
        ));
    }
    properties.push((
        "Symbol.toStringTag".to_string(),
        quench_runtime::value::Value::String("Module".to_string()),
    ));
    properties.push((
        "\0quench:descriptor:\0Symbol.toStringTag".to_string(),
        quench_runtime::value::Value::object(vec![
            (
                "value".to_string(),
                quench_runtime::value::Value::String("Module".to_string()),
            ),
            (
                "writable".to_string(),
                quench_runtime::value::Value::Boolean(false),
            ),
            (
                "enumerable".to_string(),
                quench_runtime::value::Value::Boolean(false),
            ),
            (
                "configurable".to_string(),
                quench_runtime::value::Value::Boolean(false),
            ),
        ]),
    ));
    properties.push((
        "\0quench:non_extensible".to_string(),
        quench_runtime::value::Value::Boolean(true),
    ));
    let namespace = ModuleBindingCell::new(quench_runtime::value::Value::object(properties));
    unit.namespace.replace(Some(namespace.clone()));
    Ok(namespace)
}

fn descriptor_value(unit: &LinkedModule, name: &str) -> Value {
    let value = unit
        .export_cell(name)
        .map(|cell| Value::BindingCell(cell.shared()))
        .unwrap_or(Value::Undefined);
    let mut descriptor = vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(false)),
    ];
    if unit.export_is_uninitialized(name) {
        descriptor.push(("\0quench:uninitialized".to_string(), Value::Boolean(true)));
    }
    Value::object(descriptor)
}

impl LinkedModule {
    fn instantiate(&self) -> Result<(), String> {
        let Some(end) = self
            .program
            .ops()
            .iter()
            .position(|op| matches!(op, quench_runtime::ops::Op::ModuleEvaluationStart))
        else {
            return Ok(());
        };
        let mut registers = Vec::new();
        self.scope
            .execute_completion(&self.program.ops()[..end], &mut registers, host_context())
            .map(|_| ())
            .map_err(|error| format!("residual VM error: {}", error.render()))
    }

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
            executed: Cell::new(false),
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
            executed: Cell::new(false),
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

    pub fn compile_bytes(bytes: &[u8]) -> Result<Self, String> {
        let values = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        Self::compile(&format!(
            "const buffer = new Uint8Array([{values}]).buffer.transferToImmutable(); export default new Uint8Array(buffer);"
        ))
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
        let cell = self
            .program
            .local_slots
            .get(local)
            .copied()
            .map(|slot| self.scope.module_cell_slot(slot));
        if !self.executed.get() {
            if let (Some(slot), Some(cell)) = (self.export_slot(name), cell.as_ref()) {
                if self
                .program
                .ops()
                .iter()
                .any(|op| matches!(op, quench_runtime::ops::Op::MarkUninitialized { slot: found } if *found == slot))
            {
                cell.mark_uninitialized();
            }
            }
        }
        cell
    }

    fn export_slot(&self, name: &str) -> Option<u16> {
        let local = self
            .program
            .module_metadata
            .as_ref()?
            .exports
            .iter()
            .find(|binding| binding.exported == name)?
            .local
            .as_str();
        self.program.local_slots.get(local).copied()
    }

    fn export_is_uninitialized(&self, name: &str) -> bool {
        if let Some(cell) = self.linked_exports.borrow().get(name) {
            return ModuleBindingCell::is_uninitialized(&cell.get());
        }
        let Some(slot) = self.export_slot(name) else {
            return false;
        };
        self.scope.is_uninitialized_slot(slot)
            || self.program.ops().iter().any(|op| {
                matches!(op, quench_runtime::ops::Op::MarkUninitialized { slot: found } if *found == slot)
            })
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
            self.refresh_namespace_export(name);
        } else {
            self.linked_exports.borrow_mut().remove(name);
            self.ambiguous_exports.borrow_mut().insert(name.to_string());
        }
    }

    fn refresh_namespace_export(&self, name: &str) {
        let Some(namespace) = self.namespace.borrow().as_ref().cloned() else {
            return;
        };
        let Some(cell) = self
            .linked_exports
            .borrow()
            .get(name)
            .cloned()
            .or_else(|| self.export_cell(name))
        else {
            return;
        };
        let Value::Object(properties) = namespace.get() else {
            return;
        };
        let value = Value::BindingCell(cell.shared());
        let descriptor_key = format!("\0quench:descriptor:\0{name}");
        let mut entries = properties.iter().cloned().collect::<Vec<_>>();
        let mut descriptor_entries = vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(false)),
        ];
        if self.export_is_uninitialized(name) {
            descriptor_entries.push(("\0quench:uninitialized".to_string(), Value::Boolean(true)));
        }
        let descriptor = Value::object(descriptor_entries);
        entries.retain(|(key, _)| key != name && key != &descriptor_key);
        entries.push((name.to_string(), value));
        entries.push((descriptor_key, descriptor));
        namespace.set(Value::object(entries));
    }

    fn reset_links(&self) {
        self.linked_exports.borrow_mut().clear();
        self.export_candidates.borrow_mut().clear();
        self.ambiguous_exports.borrow_mut().clear();
    }

    pub fn execute(&self) -> Result<quench_runtime::value::Value, String> {
        if self.started.replace(true) {
            return Ok(Value::Undefined);
        }
        if self.executed.get() {
            return Ok(Value::Undefined);
        }
        if self.evaluating.replace(true) {
            return Ok(Value::Undefined);
        }
        let result = self.execute_inner();
        if self.async_next.get().is_none() || result.is_err() {
            self.evaluating.set(false);
        }
        result
    }

    fn is_evaluating(&self) -> bool {
        self.evaluating.get()
    }

    fn is_executed(&self) -> bool {
        self.executed.get()
    }

    fn error_value(&self) -> Option<Value> {
        self.error.borrow().clone()
    }

    fn restore_deferred_markers(&self, id: u32) {
        let namespaces = [
            self.deferred_namespace.borrow().as_ref().cloned(),
            self.namespace.borrow().as_ref().cloned(),
        ];
        for namespace in namespaces.into_iter().flatten() {
            let Value::Object(properties) = namespace.get() else {
                continue;
            };
            for (key, value) in properties.iter() {
                if !key.starts_with("\0quench:deferred:") && key != "\0quench:deferred-module" {
                    continue;
                }
                if let Value::BindingCell(cell) = value {
                    if matches!(*cell.borrow(), Value::Undefined) {
                        cell.replace(Value::Number(f64::from(id)));
                    }
                }
            }
        }
    }

    fn clear_deferred_markers(&self) {
        let namespaces = [
            self.deferred_namespace.borrow().as_ref().cloned(),
            self.namespace.borrow().as_ref().cloned(),
        ];
        for namespace in namespaces.into_iter().flatten() {
            let Value::Object(properties) = namespace.get() else {
                continue;
            };
            for (key, value) in properties.iter() {
                if !key.starts_with("\0quench:deferred:") && key != "\0quench:deferred-module" {
                    continue;
                }
                if let Value::BindingCell(cell) = value {
                    cell.replace(Value::Undefined);
                }
            }
        }
    }

    fn execute_inner(&self) -> Result<quench_runtime::value::Value, String> {
        let start = self
            .program
            .ops()
            .iter()
            .position(|op| matches!(op, quench_runtime::ops::Op::ModuleEvaluationStart))
            .map_or(0, |index| index + 1);
        let mut registers = Vec::new();
        let result = if self.has_top_level_await() {
            self.execute_async_step(start, &mut registers)?
        } else {
            self.scope
                .execute(&self.program.ops()[start..], &mut registers, host_context())
                .map_err(|error| self.record_error(error))?
        };
        if self.async_next.get().is_some() {
            self.async_registers.replace(registers);
            return Ok(result);
        }
        self.finish_execution()?;
        Ok(result)
    }

    fn execute_async_step(
        &self,
        start: usize,
        registers: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let end = self.program.ops()[start..]
            .iter()
            .position(|op| matches!(op, quench_runtime::ops::Op::Await { .. }))
            .map_or(self.program.ops().len() - start, |index| index + 1);
        let _step_guard = quench_runtime::vm::install_async_module_step();
        let (completion, next) = self
            .scope
            .execute_completion_step(
                &self.program.ops()[start..start + end],
                registers,
                host_context(),
            )
            .map_err(|error| self.record_error(error))?;
        let stopped_at_await = end < self.program.ops().len() - start;
        match completion {
            quench_runtime::completion::Completion::Suspend(_) => {
                self.async_next.set(Some(start + next.saturating_sub(1)));
                Ok(Value::Undefined)
            }
            quench_runtime::completion::Completion::Normal if stopped_at_await => {
                self.async_next.set(Some(start + next));
                Ok(Value::Undefined)
            }
            completion => completion
                .into_vm_error()
                .map_err(|error| self.record_error(error)),
        }
    }

    fn resume_async(&self) -> Result<(), String> {
        let Some(start) = self.async_next.take() else {
            return Ok(());
        };
        let mut registers = self.async_registers.take();
        let end = self.program.ops()[start..]
            .iter()
            .position(|op| matches!(op, quench_runtime::ops::Op::Await { .. }))
            .map_or(self.program.ops().len() - start, |index| index + 1);
        let (completion, next) = self
            .scope
            .execute_resumed_completion_step(
                &self.program.ops()[start..start + end],
                &mut registers,
                host_context(),
            )
            .map_err(|error| self.record_error(error))?;
        match completion {
            quench_runtime::completion::Completion::Normal => {
                if end < self.program.ops().len() - start {
                    self.async_next.set(Some(start + next));
                    self.async_registers.replace(registers);
                } else {
                    self.finish_execution()?;
                    self.evaluating.set(false);
                }
                Ok(())
            }
            quench_runtime::completion::Completion::Suspend(_) => {
                self.async_next.set(Some(start + next));
                self.async_registers.replace(registers);
                Ok(())
            }
            completion => completion
                .into_vm_error()
                .map_err(|error| self.record_error(error))
                .map(|_| ()),
        }
    }

    fn has_top_level_await(&self) -> bool {
        self.program
            .module_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.has_top_level_await)
    }

    fn finish_execution(&self) -> Result<(), String> {
        for (name, value) in &self.fixed_exports {
            let cell = self
                .export_cell(name)
                .ok_or_else(|| format!("fixed export {name} missing"))?;
            cell.set(value.clone());
        }
        self.executed.set(true);
        self.clear_namespace_tdz();
        Ok(result)
    }

    fn clear_namespace_tdz(&self) {
        let Some(namespace) = self.namespace.borrow().as_ref().cloned() else {
            return;
        };
        let Value::Object(properties) = namespace.get() else {
            return;
        };
        let entries = properties
            .iter()
            .map(|(key, value)| {
                if key.starts_with("\0quench:descriptor:\0") {
                    let Value::Object(descriptor) = value else {
                        return (key.clone(), value.clone());
                    };
                    let filtered = descriptor
                        .iter()
                        .filter(|(name, _)| name != "\0quench:uninitialized")
                        .cloned()
                        .collect();
                    return (key.clone(), Value::object(filtered));
                }
                (key.clone(), value.clone())
            })
            .collect::<Vec<_>>();
        namespace.set(Value::object(entries));
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
        execute_program(&program, !source.contains("CanBlockIsFalse"))
    }

    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String> {
        let program =
            reduce_module_sequence(harness, source).map_err(|errors| errors.join("; "))?;
        execute_program(&program, !source.contains("CanBlockIsFalse"))
    }

    fn run_harnessed_module_at(
        &mut self,
        harness: &[&str],
        source: &str,
        path: &Path,
    ) -> Result<(), String> {
        quench_runtime::vm::reset_current_global();
        let scripts = harness
            .iter()
            .map(|source| ScriptSource {
                source,
                strict: false,
            })
            .collect::<Vec<_>>();
        let harness_program =
            reduce_script_sources(&scripts).map_err(|errors| errors.join("; "))?;
        execute_program(&harness_program, true)?;
        reduce_module_source(source).map_err(|errors| errors.join("; "))?;
        let mut graph = module_graph(path, source)?;
        let entry = graph
            .entry()
            .ok_or_else(|| "module graph missing entry".to_string())?;
        let linked = LinkedModuleGraph::compile_with_entry_prefix(&mut graph, Some(entry), &[])?;
        quench_runtime::builtins::reset_intrinsic_prototype_state();
        linked.execute(&graph, entry)
    }
}

fn module_graph(path: &Path, source: &str) -> Result<ModuleGraph, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("module {}: {error}", path.display()))?;
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(path, source.to_string());
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
        if specifier == "<module source>" {
            continue;
        }
        if graph.resolve(from, &specifier).is_some() {
            continue;
        }
        let path = base.join(&specifier);
        let dependency = add_module_source(graph, path, &metadata.import_attributes, &specifier)?;
        load_module_dependencies(graph, dependency)?;
    }
    Ok(())
}

fn module_kind(path: &Path, attributes: &[(String, String)], specifier: &str) -> ModuleKind {
    if let Some((_, attribute)) = attributes.iter().find(|(source, _)| source == specifier) {
        return match resource_type_name(attribute) {
            Some("json") => ModuleKind::Json,
            Some("text") => ModuleKind::Text,
            Some("bytes") => ModuleKind::Bytes,
            _ => ModuleKind::JavaScript,
        };
    }
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        ModuleKind::Json
    } else {
        ModuleKind::JavaScript
    }
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
    if resource_type.is_some_and(|value| resource_type_name(value) == Some("bytes")) {
        let bytes =
            std::fs::read(&path).map_err(|error| format!("module {}: {error}", path.display()))?;
        return Ok(graph.add_bytes_dependency(path, bytes));
    }
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("module {}: {error}", path.display()))?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return Ok(graph.add_json_dependency(path, source));
    }
    if resource_type.is_some_and(|value| resource_type_name(value) == Some("text")) {
        return Ok(graph.add_text_dependency(path, source));
    }
    Ok(graph.add_dependency(path, source))
}

fn resource_type_name(attribute: &str) -> Option<&str> {
    let (key, value) = attribute.split_once('=')?;
    (key == "type").then(|| value.trim_matches(['\'', '"']))
}

fn run_source(source: &str) -> Result<(), String> {
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program, !source.contains("CanBlockIsFalse"))
}

fn execute_program(
    program: &quench_runtime::reduce::ResidualProgram,
    can_block: bool,
) -> Result<(), String> {
    quench_runtime::vm::reset_host_agent_state();
    quench_runtime::builtins::reset_intrinsic_prototype_state();
    let result = execute_with_context(
        program.ops(),
        &host_context().clone().with_can_block(can_block),
    )
    .map(|_| ())
    .map_err(|error| format!("residual VM error: {}", error.render()));
    quench_runtime::vm::reset_host_agent_state();
    result
}

fn run_module_source(source: &str) -> Result<(), String> {
    let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
    execute_program(&program, !source.contains("CanBlockIsFalse"))
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
                HostCapabilityKind::Agent,
                HostCapabilityKind::AgentStart,
                HostCapabilityKind::AgentBroadcast,
                HostCapabilityKind::AgentReport,
                HostCapabilityKind::AgentGetReport,
                HostCapabilityKind::AgentLeaving,
                HostCapabilityKind::AgentReceiveBroadcast,
                HostCapabilityKind::AgentSleep,
                HostCapabilityKind::AgentTryYield,
                HostCapabilityKind::AgentTrySleep,
                HostCapabilityKind::AgentSetTimeout,
                HostCapabilityKind::AgentMonotonicNow,
            ],
        )
        .with_host_capability(
            "$262",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::GetGlobal,
            },
        )
        .with_host_capability(
            "receiveBroadcast",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::AgentReceiveBroadcast,
            },
        )
    })
}

#[cfg(test)]
mod tests {
    include!("runtime_host_tests.rs");
}
