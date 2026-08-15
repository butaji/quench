use std::path::PathBuf;

use super::{LinkedModule, LinkedModuleGraph};
use crate::module_graph::ModuleGraph;
use quench_runtime::value::Value;

#[test]
fn default_export_cell_observes_module_execution() {
    let module = LinkedModule::compile("export default true;").expect("module compiles");
    let cell = module.export_cell("default").expect("default export cell");
    module.execute().expect("module executes");
    assert_eq!(cell.get(), Value::Boolean(true));
}

#[test]
fn json_module_exports_recursive_runtime_values() {
    let module =
        LinkedModule::compile_json("[true, {\"answer\": 42}]").expect("JSON module compiles");
    let cell = module.export_cell("default").expect("default export cell");
    module.execute().expect("module executes");
    let Value::Array(values) = cell.get() else {
        panic!("JSON default export is not an array")
    };
    assert_eq!(values.logical_len(), 2);
    assert_eq!(values[0], Value::Boolean(true));
    assert!(matches!(values[1], Value::Object(_)));
}

#[test]
fn text_module_exports_source_as_string() {
    let module = LinkedModule::compile_text("export const value = 1;").expect("module compiles");
    let cell = module.export_cell("default").expect("default export cell");
    module.execute().expect("module executes");
    assert_eq!(
        cell.get(),
        Value::String("export const value = 1;".to_string())
    );
}

#[test]
fn self_text_import_uses_text_module_identity() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import value from './entry.js' with { type: 'text' }; export default typeof value;"
            .to_string(),
    );
    graph.add_text_dependency(
        PathBuf::from("entry.js"),
        "import value from './entry.js' with { type: 'text' }; export default typeof value;"
            .to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked
            .export_cell(entry, "default")
            .expect("default export")
            .get(),
        Value::String("string".to_string())
    );
}

#[test]
fn self_text_import_with_prefix_uses_text_module_identity() {
    let source =
        "import value from './entry.js' with { type: 'text' }; export default typeof value;";
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(PathBuf::from("entry.js"), source.to_string());
    graph.add_text_dependency(PathBuf::from("entry.js"), source.to_string());
    let linked = LinkedModuleGraph::compile_with_entry_prefix(&mut graph, Some(entry), &[" "])
        .expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked
            .export_cell(entry, "default")
            .expect("default export")
            .get(),
        Value::String("string".to_string())
    );
}

#[test]
fn resource_imports_survive_assert_harness_prefix() {
    let source = "import value from './entry.js' with { type: 'text' }; assert.sameValue(typeof value, 'string');";
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(PathBuf::from("entry.js"), source.to_string());
    graph.add_text_dependency(PathBuf::from("entry.js"), source.to_string());
    let linked = LinkedModuleGraph::compile_with_entry_prefix(
        &mut graph,
        Some(entry),
        &[include_str!("../../../tests/test262/harness/assert.js")],
    )
    .expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
}

#[test]
fn linked_import_reads_the_exporters_live_default_cell() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import value from './dep.js'; export default value;".to_string(),
    );
    graph.add_dependency(PathBuf::from("dep.js"), "export default true;".to_string());
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked
            .export_cell(entry, "default")
            .expect("default export")
            .get(),
        Value::Boolean(true)
    );
}

#[test]
fn namespace_import_reads_live_export_cells() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import * as ns from './dep.js'; export default ns.value;".to_string(),
    );
    graph.add_dependency(
        PathBuf::from("dep.js"),
        "export const value = true;".to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked
            .export_cell(entry, "default")
            .expect("default export")
            .get(),
        Value::Boolean(true)
    );
}

#[test]
fn export_star_forwards_live_cells() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import { value } from './barrel.js'; export default value;".to_string(),
    );
    graph.add_dependency(
        PathBuf::from("barrel.js"),
        "export * from './dep.js';".to_string(),
    );
    graph.add_dependency(
        PathBuf::from("dep.js"),
        "export const value = true;".to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked
            .export_cell(entry, "default")
            .expect("default export")
            .get(),
        Value::Boolean(true)
    );
}
