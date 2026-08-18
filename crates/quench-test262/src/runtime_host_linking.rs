fn bind_imports(
    units: &HashMap<ModuleId, LinkedModule>,
    graph: &ModuleGraph,
    order: &[ModuleId],
    accept: impl Fn(&quench_runtime::reduce::ImportBinding) -> bool,
) -> Result<(), String> {
    for id in order {
        let metadata = unit_metadata(units, *id)?;
        for binding in metadata
            .imports
            .iter()
            .filter(|binding| binding.is_binding() && accept(binding))
        {
            let target = graph
                .resolve(*id, &binding.source)
                .ok_or_else(|| format!("unresolved module {}", binding.source))?;
            let cell = import_cell(graph, units, target, &binding.imported, binding.deferred)?;
            units
                .get(id)
                .ok_or_else(|| "module unit missing".to_string())?
                .bind_import(&binding.local, cell)?;
        }
    }
    Ok(())
}

fn link_reexports(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
    id: ModuleId,
) -> Result<(), String> {
    let metadata = unit_metadata(units, id)?;
    for binding in &metadata.reexports {
        let target = resolve_reexport(graph, id, &binding.source)?;
        link_reexport(graph, units, id, target, binding)?;
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
    let cell = import_cell(graph, units, target, &binding.imported, false)?;
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
    let names = units
        .get(&target)
        .ok_or_else(|| "module unit missing".to_string())?
        .export_names();
    let from_unit = units
        .get(&from)
        .ok_or_else(|| "module unit missing".to_string())?;
    for name in names.into_iter().filter(|name| name != "default") {
        if let Some(cell) = units.get(&target).and_then(|unit| unit.export_cell(&name)) {
            if from_unit.has_local_export(&name) {
                continue;
            }
            if from_unit.has_star_export(&name) {
                if !from_unit.same_star_export(&name, &cell) {
                    from_unit.mark_ambiguous_export(&name);
                }
            } else if !from_unit.is_ambiguous_export(&name) {
                from_unit.link_star_export(&name, cell);
            }
        }
    }
    Ok(())
}

fn import_cell(
    graph: &ModuleGraph,
    units: &std::collections::HashMap<ModuleId, LinkedModule>,
    target: ModuleId,
    imported: &str,
    deferred: bool,
) -> Result<ModuleBindingCell, String> {
    if imported == "*" {
        return namespace_cell(units, target, deferred.then_some(target), deferred);
    }
    if imported == "source" {
        return units
            .get(&target)
            .map(|unit| unit.module_source.clone())
            .ok_or_else(|| "SyntaxError: module source missing".to_string());
    }
    units
        .get(&target)
        .and_then(|unit| unit.export_cell(imported))
        .or_else(|| resolve_export_cell(graph, units, target, imported, &mut HashSet::new()))
        .ok_or_else(|| format!("SyntaxError: export {imported} missing"))
}

fn resolve_export_cell(
    graph: &ModuleGraph,
    units: &HashMap<ModuleId, LinkedModule>,
    module: ModuleId,
    name: &str,
    seen: &mut HashSet<(ModuleId, String)>,
) -> Option<ModuleBindingCell> {
    if !seen.insert((module, name.to_string())) {
        return None;
    }
    if let Some(cell) = units.get(&module).and_then(|unit| unit.export_cell(name)) {
        return Some(cell);
    }
    let metadata = unit_metadata(units, module).ok()?;
    let mut result: Option<ModuleBindingCell> = None;
    for binding in &metadata.reexports {
        if binding.imported == "*all*" && name == "default" {
            continue;
        }
        if binding.imported != "*all*" && binding.exported != name {
            continue;
        }
        let target = graph.resolve(module, &binding.source)?;
        let imported = if binding.imported == "*all*" {
            name
        } else {
            &binding.imported
        };
        let mut branch_seen = seen.clone();
        let Some(cell) = resolve_export_cell(graph, units, target, imported, &mut branch_seen)
            .or_else(|| units.get(&target).and_then(|unit| unit.export_cell(imported)))
        else {
            continue;
        };
        if result
            .as_ref()
            .is_some_and(|existing| !Rc::ptr_eq(&existing.shared(), &cell.shared()))
        {
            return None;
        }
        result = Some(cell);
    }
    result
}

fn namespace_cell(
    units: &std::collections::HashMap<ModuleId, LinkedModule>,
    target: ModuleId,
    evaluate: Option<ModuleId>,
    deferred: bool,
) -> Result<ModuleBindingCell, String> {
    let unit = units
        .get(&target)
        .ok_or_else(|| "module unit missing".to_string())?;
    let cached = if deferred {
        unit.deferred_namespace_cell.borrow().clone()
    } else {
        unit.namespace_cell.borrow().clone()
    };
    if let Some(cell) = cached {
        if !deferred {
            unit.refresh_namespace();
        }
        return Ok(cell);
    }
    let mut export_names = unit.export_names();
    export_names.sort();
    let mut properties: Vec<(String, quench_runtime::value::Value)> = export_names
        .iter()
        .filter_map(|name| unit.export_cell(name).map(|cell| (name.clone(), cell)))
        .map(|(name, cell)| {
            (
                name,
                quench_runtime::value::Value::BindingCell(cell.shared()),
            )
        })
        .collect();
    properties.push((
        "\0prototype".to_string(),
        quench_runtime::value::Value::Null,
    ));
    properties.push((
        "\0quench:non_extensible".to_string(),
        quench_runtime::value::Value::Boolean(true),
    ));
    quench_runtime::module_bindings::mark_namespace(&mut properties);
    let tag = if deferred {
        "Deferred Module"
    } else {
        "Module"
    };
    properties.push((
        "Symbol.toStringTag".to_string(),
        quench_runtime::value::Value::String(tag.to_string()),
    ));
    properties.push((
        "\0quench:descriptor:\0Symbol.toStringTag".to_string(),
        quench_runtime::value::Value::object(vec![
            (
                "value".to_string(),
                quench_runtime::value::Value::String(tag.to_string()),
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
    for name in unit.export_names() {
        if let Some(cell) = unit.export_cell(&name) {
            properties.push((
                format!("\0quench:descriptor:\0{name}"),
                quench_runtime::value::Value::object(vec![
                    (
                        "value".to_string(),
                        quench_runtime::value::Value::BindingCell(cell.shared()),
                    ),
                    (
                        "writable".to_string(),
                        quench_runtime::value::Value::Boolean(true),
                    ),
                    (
                        "enumerable".to_string(),
                        quench_runtime::value::Value::Boolean(true),
                    ),
                    (
                        "configurable".to_string(),
                        quench_runtime::value::Value::Boolean(false),
                    ),
                ]),
            ));
        }
    }
    let cell = ModuleBindingCell::new(quench_runtime::value::Value::object(properties));
    if let Some(id) = evaluate {
        quench_runtime::module_bindings::attach_evaluator(
            &cell.get(),
            std::rc::Rc::new(move || ensure_module(id)),
        );
    }
    if deferred {
        *unit.deferred_namespace_cell.borrow_mut() = Some(cell.clone());
    } else {
        *unit.namespace_cell.borrow_mut() = Some(cell.clone());
    }
    Ok(cell)
}


