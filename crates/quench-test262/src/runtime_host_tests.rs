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
    let module = LinkedModule::compile_json("[true, {\"answer\": 42}]").expect("JSON module compiles");
    let cell = module.export_cell("default").expect("default export cell");
    module.execute().expect("module executes");
    let Value::Array(values) = cell.get() else { panic!("JSON default export is not an array") };
    assert_eq!(values.logical_len(), 2);
    assert_eq!(values[0], Value::Boolean(true));
    assert!(matches!(values[1], Value::Object(_)));
}

#[test]
fn linked_import_reads_the_exporters_live_default_cell() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(PathBuf::from("entry.js"), "import value from './dep.js'; export default value;".to_string());
    graph.add_dependency(PathBuf::from("dep.js"), "export default true;".to_string());
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(linked.export_cell(entry, "default").expect("default export").get(), Value::Boolean(true));
}

#[test]
fn namespace_import_reads_live_export_cells() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(PathBuf::from("entry.js"), "import * as ns from './dep.js'; export default ns.value;".to_string());
    graph.add_dependency(PathBuf::from("dep.js"), "export const value = true;".to_string());
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(linked.export_cell(entry, "default").expect("default export").get(), Value::Boolean(true));
}

#[test]
fn text_import_attribute_does_not_parse_fixture_as_javascript() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import value from './note.js' with { type: 'text' };\nexport default value;\n".to_string(),
    );
    graph.add_text_dependency(PathBuf::from("note.js"), "invalid { javascript".to_string());
    graph.link(entry, graph.resolve(entry, "./note.js").expect("resolved"))
        .expect("linked");
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked.export_cell(entry, "default").expect("default").get(),
        Value::String("invalid { javascript".to_string())
    );
}

#[test]
fn modules_share_global_this_across_units() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import './setup.js';\nexport default globalThis.evaluations.length;\n".to_string(),
    );
    graph.add_dependency(
        PathBuf::from("setup.js"),
        "globalThis.evaluations = [];\n".to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked.export_cell(entry, "default").expect("default").get(),
        Value::Number(0.0)
    );
}

#[test]
fn import_defer_does_not_evaluate_dependency() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import defer * as ns from './dep.js';\nexport default 1;\n".to_string(),
    );
    let dep = graph.add_dependency(
        PathBuf::from("dep.js"),
        "export const value = 2;\n".to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked.export_cell(entry, "default").expect("default").get(),
        Value::Number(1.0)
    );
    let value = linked.export_cell(dep, "value").expect("dep export").get();
    assert!(
        matches!(value, Value::Undefined)
            || quench_runtime::module_bindings::ModuleBindingCell::is_unresolved(&value),
        "deferred module must not evaluate: {value:?}"
    );
}

#[test]
fn deferred_export_cell_is_live_after_namespace_get() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        "import defer * as ns from './dep.js';\nexport default ns.foo;\n".to_string(),
    );
    let dep = graph.add_dependency(
        PathBuf::from("dep.js"),
        "export const foo = 1;\n".to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked.export_cell(dep, "foo").expect("foo").get(),
        Value::Number(1.0),
        "export cell after deferred evaluation"
    );
    assert_eq!(
        linked.export_cell(entry, "default").expect("default").get(),
        Value::Number(1.0),
        "ns.foo after deferred evaluation"
    );
}

#[test]
fn deferred_gopd_after_own_keys() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        concat!(
            "import defer * as ns from './dep.js';\n",
            "const keys = Reflect.ownKeys(ns);\n",
            "const desc = Reflect.getOwnPropertyDescriptor(ns, 'foo');\n",
            "export default desc && desc.value;\n",
        )
        .to_string(),
    );
    graph.add_dependency(PathBuf::from("dep.js"), "export const foo = 1;\nexport const bar = 2;\n".to_string());
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked.export_cell(entry, "default").expect("default").get(),
        Value::Number(1.0)
    );
}

#[test]
fn export_star_forwards_live_cells() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(PathBuf::from("entry.js"), "import { value } from './barrel.js'; export default value;".to_string());
    graph.add_dependency(PathBuf::from("barrel.js"), "export * from './dep.js';".to_string());
    graph.add_dependency(PathBuf::from("dep.js"), "export const value = true;".to_string());
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(linked.export_cell(entry, "default").expect("default export").get(), Value::Boolean(true));
}
