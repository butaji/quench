fn wake_waiting_modules() {
    CURRENT_MODULE_GRAPH.with(|current| {
        let Some((units, graph)) = current.get() else {
            return;
        };
        let units = unsafe { &*units };
        let graph = unsafe { &*graph };
        for id in units.units.keys().copied() {
            let Some(unit) = units.units.get(&id) else {
                continue;
            };
            if unit.started.get() && !unit.evaluated.get() && !unit.async_suspended.get() {
                let units = units as *const LinkedModuleGraph;
                let modules = graph as *const ModuleGraph;
                quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
                    let _ = evaluate_module(unsafe { &*units }, unsafe { &*modules }, id, true);
                }));
            }
        }
    });
}

fn settle_dynamic_import(graph_units: &LinkedModuleGraph, id: ModuleId) {
    for _ in 0..1024 {
        let Some(unit) = graph_units.units.get(&id) else {
            return;
        };
        if unit.evaluated.get() || unit.thrown.borrow().is_some() {
            if let Some(thrown) = unit.thrown.borrow().clone() {
                quench_runtime::module_bindings::request_ensure_throw(thrown);
            }
            return;
        }
        quench_runtime::module_bindings::drain_jobs();
    }
}

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
        if unit.async_suspended.get() {
            return Ok(());
        }
        return continue_started_module(graph_units, graph, id, skip_deferred);
    }
    start_module(graph_units, graph, id, skip_deferred)
}

fn start_module(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    skip_deferred: bool,
) -> Result<(), String> {
    let unit = graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?;
    unit.started.set(true);
    let metadata = unit.program.module_metadata.as_ref();
    evaluate_request_wave(graph_units, graph, id, metadata, skip_deferred)?;
    if unfinished_targets(graph_units, metadata, graph, id, skip_deferred) {
        schedule_evaluate(graph_units, graph, id);
        return Ok(());
    }
    run_module_body(graph_units, id)
}

fn schedule_evaluate(graph_units: &LinkedModuleGraph, graph: &ModuleGraph, id: ModuleId) {
    let units = graph_units as *const LinkedModuleGraph;
    let modules = graph as *const ModuleGraph;
    quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
        let _ = evaluate_module(unsafe { &*units }, unsafe { &*modules }, id, true);
    }));
}

fn run_module_body(graph_units: &LinkedModuleGraph, id: ModuleId) -> Result<(), String> {
    CURRENT_MODULE_ID.with(|current| current.set(Some(id)));
    let result = graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?
        .execute();
    CURRENT_MODULE_ID.with(|current| current.set(None));
    result.map(|_| ())
}

fn continue_started_module(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    id: ModuleId,
    skip_deferred: bool,
) -> Result<(), String> {
    let unit = graph_units
        .units
        .get(&id)
        .ok_or_else(|| "module unit missing".to_string())?;
    if unit.evaluated.get() {
        return Ok(());
    }
    let metadata = unit.program.module_metadata.as_ref();
    let targets = evaluation_targets(graph_units, metadata, graph, id, skip_deferred);
    if let Some(thrown) = targets.iter().find_map(|dependency| {
        graph_units
            .units
            .get(dependency)
            .and_then(|unit| unit.thrown.borrow().clone())
    }) {
        *unit.thrown.borrow_mut() = Some(thrown.clone());
        quench_runtime::module_bindings::request_ensure_throw(thrown);
        unit.evaluated.set(true);
        return Err("module evaluation failed".to_string());
    }
    if targets.iter().any(|dependency| {
        graph_units
            .units
            .get(dependency)
            .is_some_and(|unit| !unit.evaluated.get())
    }) {
        return Ok(());
    }
    CURRENT_MODULE_ID.with(|current| current.set(Some(id)));
    let result = unit.execute();
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

fn unfinished_targets(
    units: &LinkedModuleGraph,
    metadata: Option<&quench_runtime::reduce::ModuleMetadata>,
    graph: &ModuleGraph,
    from: ModuleId,
    skip_deferred: bool,
) -> bool {
    evaluation_targets(units, metadata, graph, from, skip_deferred)
        .iter()
        .any(|dependency| {
            units
                .units
                .get(dependency)
                .is_some_and(|unit| !unit.evaluated.get())
        })
}

fn evaluate_request_wave(
    graph_units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    from: ModuleId,
    metadata: Option<&quench_runtime::reduce::ModuleMetadata>,
    skip_deferred: bool,
) -> Result<(), String> {
    let (first, delayed) = partition_evaluation(graph_units, graph, from, metadata, skip_deferred);
    for dependency in &first {
        evaluate_module(graph_units, graph, *dependency, skip_deferred)?;
    }
    if !delayed.is_empty()
        || first.iter().any(|id| {
            graph_units
                .units
                .get(id)
                .is_some_and(|unit| !unit.evaluated.get())
        })
    {
        quench_runtime::module_bindings::drain_jobs();
    }
    if !delayed.is_empty() {
        for dependency in delayed {
            evaluate_module(graph_units, graph, dependency, skip_deferred)?;
        }
        quench_runtime::module_bindings::drain_jobs();
    }
    Ok(())
}

fn partition_evaluation(
    units: &LinkedModuleGraph,
    graph: &ModuleGraph,
    from: ModuleId,
    metadata: Option<&quench_runtime::reduce::ModuleMetadata>,
    skip_deferred: bool,
) -> (Vec<ModuleId>, Vec<ModuleId>) {
    let Some(metadata) = metadata else {
        return (graph.dependencies(from).to_vec(), Vec::new());
    };
    let mut first = Vec::new();
    let mut delayed = Vec::new();
    let mut pending_tla = false;
    for request in &metadata.requests {
        let Some(target) = graph.resolve(from, &request.source) else {
            continue;
        };
        if skip_deferred && request.deferred {
            let mut gat = Vec::new();
            gather_async_transitive(units, graph, target, &mut HashSet::new(), &mut gat);
            if pending_tla {
                delayed.extend(gat);
            } else {
                pending_tla = !gat.is_empty();
                first.extend(gat);
            }
            continue;
        }
        if !first.contains(&target) {
            first.push(target);
        }
    }
    (first, delayed)
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


