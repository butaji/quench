fn ensure_module(id: ModuleId) {
    CURRENT_MODULE_GRAPH.with(|current| {
        let Some((units, graph)) = current.get() else {
            return;
        };
        let units = unsafe { &*units };
        let graph = unsafe { &*graph };
        if !ready_for_sync_execution(units, graph, id, &mut HashSet::new()) {
            quench_runtime::module_bindings::request_ensure_type_error();
            return;
        }
        let _ = evaluate_module(units, graph, id, true);
    });
}

fn evaluate_module(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    skip_deferred: bool,
) -> Result<(), String> {
    let unit = graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?;
    if let Some(thrown) = unit.thrown.borrow().clone() {
        quench_runtime::module_bindings::request_ensure_throw(thrown);
        return Err("module evaluation failed".to_string());
    }
    if unit.started.get() {
        return Ok(());
    }
    unit.started.set(true);
    let metadata = unit.program.module_metadata.as_ref();
    for dependency in evaluation_targets(metadata, graph, id, skip_deferred) {
        evaluate_module(graph_units, graph, dependency, skip_deferred)?;
    }
    gather_deferred_async(graph_units, graph, id, metadata, skip_deferred)?;
    graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?
        .execute()?;
    Ok(())
}

fn gather_deferred_async(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    metadata: Option<&quench_runtime::reduce::ModuleMetadata>,
    skip_deferred: bool,
) -> Result<(), String> {
    if !skip_deferred {
        return Ok(());
    }
    let Some(metadata) = metadata else {
        return Ok(());
    };
    for binding in metadata.imports.iter().filter(|binding| binding.deferred) {
        if let Some(target) = graph.resolve(id, &binding.source) {
            evaluate_async_transitive(graph_units, graph, target, &mut HashSet::new())?;
        }
    }
    Ok(())
}

fn evaluate_async_transitive(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    seen: &mut HashSet<ModuleId>,
) -> Result<(), String> {
    if !seen.insert(id) {
        return Ok(());
    }
    let unit = graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?;
    if unit
        .program
        .module_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.has_top_level_await)
    {
        return evaluate_module(graph_units, graph, id, true);
    }
    for dependency in graph.dependencies(id) {
        evaluate_async_transitive(graph_units, graph, *dependency, seen)?;
    }
    Ok(())
}

fn ready_for_sync_execution(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    seen: &mut HashSet<ModuleId>,
) -> bool {
    if !seen.insert(id) {
        return true;
    }
    let Some(unit) = graph_units.units.get(&id) else {
        return false;
    };
    if unit.evaluated.get() {
        return true;
    }
    if unit.started.get() {
        return false;
    }
    if unit
        .program
        .module_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.has_top_level_await)
    {
        return false;
    }
    graph
        .dependencies(id)
        .iter()
        .all(|dependency| ready_for_sync_execution(graph_units, graph, *dependency, seen))
}

fn evaluation_targets(
    metadata: Option<&quench_runtime::reduce::ModuleMetadata>,
    graph: &ModuleGraph,
    from: ModuleId,
    skip_deferred: bool,
) -> Vec<ModuleId> {
    let Some(metadata) = metadata else {
        return graph.dependencies(from).to_vec();
    };
    let mut targets = Vec::new();
    for binding in &metadata.imports {
        if skip_deferred && binding.deferred {
            continue;
        }
        push_target(&mut targets, graph, from, &binding.source);
    }
    for binding in &metadata.reexports {
        push_target(&mut targets, graph, from, &binding.source);
    }
    targets
}

fn push_target(targets: &mut Vec<ModuleId>, graph: &ModuleGraph, from: ModuleId, source: &str) {
    let Some(target) = graph.resolve(from, source) else {
        return;
    };
    if !targets.contains(&target) {
        targets.push(target);
    }
}
