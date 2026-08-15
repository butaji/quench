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
    deferred_namespace: RefCell<Option<ModuleBindingCell>>,
    executed: Cell<bool>,
    evaluating: Cell<bool>,
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
        bind_imports(graph, &units, true)?;
        for unit in units.values() {
            unit.reset_links();
        }
        link_reexports(graph, &units)?;
        link_reexports(graph, &units)?;
        link_reexports(graph, &units)?;
        bind_imports(graph, &units, false)?;
        Ok(Self { units })
    }

    pub fn execute(&self, graph: &ModuleGraph, entry: ModuleId) -> Result<(), String> {
        let graph_ptr = self as *const Self;
        let module_graph_ptr = graph as *const ModuleGraph;
        let _callback = quench_runtime::vm::install_deferred_module_callback(Rc::new(move |id| {
            // The callback guard lives only for the duration of this execution.
            // Both pointed-to graphs are borrowed by the caller for that duration.
            let target = ModuleId(id);
            unsafe { (&*graph_ptr).execute(&*module_graph_ptr, target) }
                .map(|_| {
                    if let Some(unit) = unsafe { (&*graph_ptr).units.get(&target) } {
                        unit.complete_deferred_namespace();
                    }
                    Value::Undefined
                })
                .map_err(quench_runtime::execute::VmError::EvalError)
        }));
        let graph_ptr = self as *const Self;
        let module_graph_ptr = graph as *const ModuleGraph;
        let _dynamic = quench_runtime::vm::install_dynamic_import_callback(Rc::new(
            move |specifier, deferred, options| {
                let graph = unsafe { &*module_graph_ptr };
                let kind = dynamic_module_kind(&options);
                let target = graph
                    .resolve_kind(entry, &specifier, kind)
                    .or_else(|| graph.resolve_kind(entry, &specifier, ModuleKind::Json))
                    .or_else(|| graph.resolve_kind(entry, &specifier, ModuleKind::Text))
                    .or_else(|| graph.resolve_kind(entry, &specifier, ModuleKind::JavaScript))
                    .ok_or_else(|| {
                        quench_runtime::execute::VmError::EvalError(
                            "Cannot resolve dynamic import".to_string(),
                        )
                    })?;
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
                namespace_cell(unsafe { &(*graph_ptr).units }, target, deferred)
                    .map(|cell| cell.get())
                    .map_err(quench_runtime::execute::VmError::EvalError)
            },
        ));
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

fn dynamic_import_error(error: &str) -> Value {
    let message = error
        .strip_prefix("residual VM error: TypeError: ")
        .unwrap_or(error);
    quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(message.to_string())],
    )
}

fn dynamic_module_kind(options: &Value) -> ModuleKind {
    let Value::Object(properties) = options else {
        return ModuleKind::JavaScript;
    };
    let Some(Value::Object(with)) = properties
        .iter()
        .find(|(key, _)| key == "with")
        .map(|(_, value)| value)
    else {
        return ModuleKind::JavaScript;
    };
    let Some(Value::String(kind)) = with
        .iter()
        .find(|(key, _)| key == "type")
        .map(|(_, value)| value)
    else {
        return ModuleKind::JavaScript;
    };
    match kind.as_str() {
        "json" => ModuleKind::Json,
        "text" => ModuleKind::Text,
        "bytes" => ModuleKind::Bytes,
        _ => ModuleKind::JavaScript,
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
            let kind = import_kind(metadata, &binding.source);
            let target = graph
                .resolve_kind(id, &binding.source, kind)
                .ok_or_else(|| format!("unresolved module {}", binding.source))?;
            let deferred = binding.deferred;
            let cell = match import_cell(units, target, &binding.imported, provisional, deferred) {
                Ok(cell) => cell,
                Err(_error) if provisional && binding.imported != "*" => continue,
                Err(error) => return Err(error),
            };
            units
                .get(&id)
                .ok_or_else(|| "module unit missing".to_string())?
                .bind_import(&binding.local, cell)?;
        }
    }
    Ok(())
}

fn import_kind(metadata: &quench_runtime::reduce::ModuleMetadata, source: &str) -> ModuleKind {
    metadata
        .import_attributes
        .iter()
        .find(|(specifier, _)| specifier == source)
        .and_then(|(_, value)| match value.as_str() {
            "type=json" => Some(ModuleKind::Json),
            "type=text" => Some(ModuleKind::Text),
            "type=bytes" => Some(ModuleKind::Bytes),
            _ => None,
        })
        .unwrap_or(ModuleKind::JavaScript)
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
    let cell = match import_cell(units, target, &binding.imported, true, false) {
        Ok(cell) => cell,
        Err(error) if declares_export(units, target, &binding.imported) => {
            let _ = error;
            ModuleBindingCell::unresolved()
        }
        Err(error) => return Err(error),
    };
    units
        .get(&from)
        .ok_or_else(|| "module unit missing".to_string())?
        .link_export(&binding.exported, cell);
    Ok(())
}

fn declares_export(units: &HashMap<ModuleId, LinkedModule>, target: ModuleId, name: &str) -> bool {
    let Some(metadata) = units
        .get(&target)
        .and_then(|unit| unit.program.module_metadata.as_ref())
    else {
        return false;
    };
    metadata
        .exported_names
        .iter()
        .any(|exported| exported == name)
        || metadata
            .reexports
            .iter()
            .any(|binding| binding.exported == name)
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
    deferred: bool,
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
    let cache = if deferred {
        &unit.deferred_namespace
    } else {
        &unit.namespace
    };
    if let Some(namespace) = cache.borrow().as_ref() {
        return Ok(namespace.clone());
    }
    let mut properties = vec![(
        "\0prototype".to_string(),
        quench_runtime::value::Value::Null,
    )];
    let mut export_names = unit.export_names();
    export_names.retain(|name| !unit.ambiguous_exports.borrow().contains(name));
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
        if deferred {
            properties.push((
                format!("\0quench:deferred:\0{name}"),
                Value::BindingCell(Rc::new(RefCell::new(Value::Number(f64::from(target.0))))),
            ));
        }
    }
    if deferred {
        properties.push((
            "\0quench:deferred-module".to_string(),
            Value::BindingCell(Rc::new(RefCell::new(Value::Number(f64::from(target.0))))),
        ));
    }
    let tag_value = if deferred {
        Value::BindingCell(Rc::new(RefCell::new(Value::String(
            "Deferred Module".to_string(),
        ))))
    } else {
        Value::String("Module".to_string())
    };
    properties.push(("Symbol.toStringTag".to_string(), tag_value.clone()));
    properties.push((
        "\0quench:descriptor:\0Symbol.toStringTag".to_string(),
        quench_runtime::value::Value::object(vec![
            ("value".to_string(), tag_value),
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
    cache.replace(Some(namespace.clone()));
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
            deferred_namespace: RefCell::new(None),
            executed: Cell::new(false),
            evaluating: Cell::new(false),
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
            deferred_namespace: RefCell::new(None),
            executed: Cell::new(false),
            evaluating: Cell::new(false),
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
        if let Some(existing) = entries.first() {
            if ModuleBindingCell::is_unresolved(&existing.get()) {
                existing.forward_to(&cell);
                self.linked_exports
                    .borrow_mut()
                    .insert(name.to_string(), existing.clone());
                return;
            }
        }
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
            self.remove_namespace_export(name);
        }
    }

    fn remove_namespace_export(&self, name: &str) {
        let Some(namespace) = self.namespace.borrow().as_ref().cloned() else {
            return;
        };
        let Value::Object(properties) = namespace.get() else {
            return;
        };
        let descriptor = format!("\0quench:descriptor:\0{name}");
        let entries = properties
            .iter()
            .filter(|(key, _)| key != name && key != &descriptor)
            .cloned()
            .collect::<Vec<_>>();
        namespace.set(Value::object(entries));
    }

    fn refresh_namespace_export(&self, name: &str) {
        let Some(namespace) = self.namespace.borrow().as_ref().cloned() else {
            return;
        };
        let Some(cell) = self.linked_exports.borrow().get(name).cloned() else {
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

    fn complete_deferred_namespace(&self) {
        let Some(namespace) = self.deferred_namespace.borrow().as_ref().cloned() else {
            return;
        };
        let Value::Object(properties) = namespace.get() else {
            return;
        };
        for (key, value) in properties.iter() {
            if key == "Symbol.toStringTag" {
                if let Value::BindingCell(cell) = value {
                    cell.replace(Value::String("Module".to_string()));
                }
            }
        }
        self.clear_namespace_tdz();
    }

    pub fn execute(&self) -> Result<quench_runtime::value::Value, String> {
        if self.evaluating.replace(true) {
            return Ok(Value::Undefined);
        }
        let result = self.execute_inner();
        self.evaluating.set(false);
        result
    }

    fn execute_inner(&self) -> Result<quench_runtime::value::Value, String> {
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
        self.executed.set(true);
        self.clear_namespace_tdz();
        Ok(result)
    }

    fn clear_namespace_tdz(&self) {
        let namespaces = [
            self.namespace.borrow().as_ref().cloned(),
            self.deferred_namespace.borrow().as_ref().cloned(),
        ];
        for namespace in namespaces.into_iter().flatten() {
            self.clear_namespace_tdz_value(namespace);
        }
    }

    fn clear_namespace_tdz_value(&self, namespace: ModuleBindingCell) {
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
        execute_program(&harness_program)?;
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
        let kind = module_kind(
            &base.join(&specifier),
            &metadata.import_attributes,
            &specifier,
        );
        if graph.resolve_kind(from, &specifier, kind).is_some() {
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
                HostCapabilityKind::DeferredModule,
                HostCapabilityKind::DynamicImport,
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
