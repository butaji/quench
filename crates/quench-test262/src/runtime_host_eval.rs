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
    let targets = evaluation_targets(graph_units, metadata, graph, id, skip_deferred);
    for dependency in &targets {
        evaluate_module(graph_units, graph, *dependency, skip_deferred)?;
    }
    if targets.iter().any(|dependency| {
        graph_units
            .units
            .get(dependency)
            .is_some_and(|unit| !unit.evaluated.get())
    }) {
        let units = graph_units as *const LinkedModuleGraph;
        quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
            let _ = unsafe { &*units }.units.get(&id).map(LinkedModule::execute);
        }));
        return Ok(());
    }
    if metadata.is_some_and(|metadata| metadata.imports.iter().any(|binding| binding.deferred)) {
        quench_runtime::module_bindings::drain_jobs();
    }
    CURRENT_MODULE_ID.with(|current| current.set(Some(id)));
    let result = graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?
        .execute();
    CURRENT_MODULE_ID.with(|current| current.set(None));
    result.map(|_| ())
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
    units: &LinkedModuleGraph,
    metadata: Option<&quench_runtime::reduce::ModuleMetadata>,
    graph: &ModuleGraph,
    from: ModuleId,
    skip_deferred: bool,
) -> Vec<ModuleId> {
    let Some(metadata) = metadata else {
        return graph.dependencies(from).to_vec();
    };
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for request in &metadata.requests {
        let Some(target) = graph.resolve(from, &request.source) else {
            continue;
        };
        if skip_deferred && request.deferred {
            gather_async_transitive(units, graph, target, &mut seen, &mut targets);
            continue;
        }
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

fn gather_async_transitive(
    units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    seen: &mut HashSet<ModuleId>,
    result: &mut Vec<ModuleId>,
) {
    if !seen.insert(id) {
        return;
    }
    let Some(unit) = units.units.get(&id) else {
        return;
    };
    if unit.started.get() || unit.evaluated.get() {
        return;
    }
    if unit
        .program
        .module_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.has_top_level_await)
    {
        if !result.contains(&id) {
            result.push(id);
        }
        return;
    }
    let Some(metadata) = unit.program.module_metadata.as_ref() else {
        return;
    };
    for source in metadata
        .imports
        .iter()
        .map(|binding| binding.source.as_str())
        .chain(metadata.reexports.iter().map(|binding| binding.source.as_str()))
    {
        if let Some(target) = graph.resolve(id, source) {
            gather_async_transitive(units, graph, target, seen, result);
        }
    }
}


