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
    if graph.unit(from).is_some_and(|unit| {
        matches!(
            unit.kind,
            ModuleKind::Json | ModuleKind::Text | ModuleKind::Bytes
        )
    }) {
        return Ok(());
    }
    let metadata = inspect_module_source(&source).map_err(|errors| errors.join("; "))?;
    for specifier in metadata.import_specifiers.clone() {
        let text_import = metadata
            .import_types
            .iter()
            .any(|(source, attribute)| source == &specifier && attribute == "type=text");
        let bytes_import = metadata
            .import_types
            .iter()
            .any(|(source, attribute)| source == &specifier && attribute == "type=bytes");
        if graph.resolve(from, &specifier).is_some() && !text_import && !bytes_import {
            continue;
        }
        if specifier == "<module source>" {
            let dependency = graph
                .add_text_dependency(Path::new("<module source>").to_path_buf(), String::new());
            graph.link(from, dependency)?;
            continue;
        }
        let path = base.join(&specifier);
        let dependency = add_module_source(graph, path, &metadata.import_types, &specifier)?;
        load_module_dependencies(graph, dependency)?;
    }
    Ok(())
}

fn add_module_source(
    graph: &mut ModuleGraph,
    path: std::path::PathBuf,
    import_types: &[(String, String)],
    specifier: &str,
) -> Result<ModuleId, String> {
    if import_types
        .iter()
        .any(|(source, attribute)| source == specifier && attribute == "type=bytes")
    {
        let bytes =
            std::fs::read(&path).map_err(|error| format!("module {}: {error}", path.display()))?;
        return Ok(graph.add_bytes_dependency(path, bytes));
    }
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("module {}: {error}", path.display()))?;
    if import_types
        .iter()
        .any(|(source, attribute)| source == specifier && attribute == "type=text")
    {
        return Ok(graph.add_text_dependency(path, source));
    }
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return Ok(graph.add_json_dependency(path, source));
    }
    Ok(graph.add_dependency(path, source))
}


