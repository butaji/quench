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
fn uninitialized_export_cell_is_marked_before_module_execution() {
    let module = LinkedModule::compile("export let value;").expect("module compiles");
    assert!(module
        .program
        .ops()
        .iter()
        .any(|op| matches!(op, quench_runtime::ops::Op::MarkUninitialized { .. })));
    let slot = module.export_slot("value").expect("export slot");
    assert!(module.program.ops().iter().any(|op| {
        matches!(op, quench_runtime::ops::Op::MarkUninitialized { slot: found } if *found == slot)
    }));
    let cell = module.export_cell("value").expect("export cell");
    assert!(quench_runtime::module_bindings::ModuleBindingCell::is_uninitialized(&cell.get()));
}

#[test]
fn named_default_function_has_a_default_export_cell() {
    let module = LinkedModule::compile("export default function f() { return 1; }")
        .expect("module compiles");
    let cell = module.export_cell("default").expect("default export cell");
    module.execute().expect("module executes");
    assert!(matches!(cell.get(), Value::Function(_)));
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
fn export_star_forwards_live_cells() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(PathBuf::from("entry.js"), "import { value } from './barrel.js'; export default value;".to_string());
    graph.add_dependency(PathBuf::from("barrel.js"), "export * from './dep.js';".to_string());
    graph.add_dependency(PathBuf::from("dep.js"), "export const value = true;".to_string());
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(linked.export_cell(entry, "default").expect("default export").get(), Value::Boolean(true));
}
