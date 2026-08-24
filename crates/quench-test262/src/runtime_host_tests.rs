use std::path::PathBuf;

use super::{LinkedModule, LinkedModuleGraph};
use crate::Test262Host;
use crate::module_graph::ModuleGraph;
use quench_runtime::value::Value;

#[test]
fn stage_report_requires_complete_path_coverage() {
    let paths = vec![PathBuf::from("a.js"), PathBuf::from("b.js")];
    let complete = crate::StageReport {
        total: 2,
        passed: 1,
        failed: 1,
        failures: vec![(paths[1].clone(), "failure".to_string())],
    };
    assert!(complete.covers(&paths));
    assert!(!crate::StageReport { total: 1, ..complete }.covers(&paths));
}

#[test]
fn sequential_script_units_share_global_function_bindings() {
    let mut host = super::RuntimeHost;
    host.run_harnessed_script(
        &["function Test262Error(message) { this.message = message; }"] ,
        "if (typeof Test262Error !== 'function') throw new Error('missing global function');",
        false,
    )
    .expect("sequential harness scripts must share global function bindings");
}

#[test]
fn strict_non_extensible_write_throws() {
    let mut host = super::RuntimeHost;
    let result = host.run_harnessed_script(
        &[],
        "var obj = {}; Object.preventExtensions(obj); obj.missing = 1;",
        true,
    );
    assert!(result.is_err(), "strict write must reject a new property");
}

#[test]
fn deleted_global_property_keeps_strict_reference_error_semantics() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var count = 0; var global = this;\n\
         Object.defineProperty(this, 'x', { configurable: true, value: 1 });\n\
         (function() {\n\
           'use strict';\n\
           var threw = false;\n\
           try { count++; x = (delete global.x, 2); count++; }\n\
           catch (error) { if (!(error instanceof ReferenceError)) throw error; threw = true; }\n\
           if (!threw) throw new Error('missing ReferenceError');\n\
           count++;\n\
         })();\n\
         if (count !== 2 || ('x' in this) || ('x' in global)) throw new Error('stale global alias');",
    )
    .expect("global aliases must follow copy-on-write replacement");
}

#[test]
fn direct_eval_parameter_var_reaches_captured_function() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var x = 'outside'; var probe;\n\
         (function(_ = (eval(\"var x = 'inside';\"), probe = function() { return x; })) {})();\n\
         if (probe() !== 'inside') throw new Error('eval parameter binding');",
    )
    .expect("direct eval var bindings must follow captured cells");
}

#[test]
fn with_global_copy_on_write_updates_the_active_realm() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var count = 0; (function() { with (this) { count++; } })();\n\
         if (count !== 1) throw new Error('with global replacement');",
    )
    .expect("with writes must publish the active global replacement");
}

#[test]
fn async_with_nested_function_runs_before_returning_promise() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var count = 0; async function f() { count++; (function() { count++; with (this) { count++; } })(); }\n\
         f(); if (count !== 3) throw new Error('async with ordering');",
    )
    .expect("async function body must run synchronously through nested with calls");
}

#[test]
fn cross_realm_class_evaluations_keep_private_static_brands_distinct() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var first = $262.createRealm(); var second = $262.createRealm();\n\
         var source1 = '(class { static #value = 1; static read() { return this.#value; } })';\n\
         var source2 = '(class { static #value = 2; static read() { return this.#value; } })';\n\
         var C1 = first.evalScript(source1); var C2 = second.evalScript(source2);\n\
         if (C1 === C2 || C1.read() !== 1 || C2.read() !== 2) throw new Error('private static setup');\n\
         var threw = false;\n\
         try { C1.read.call(C2); } catch (error) { if (!(error instanceof first.global.TypeError)) throw error; threw = true; }\n\
         if (!threw) throw new Error('cross-realm private brand');",
    )
    .expect("private static names must remain distinct across realm evaluations");
}

#[test]
fn symbols_are_unique_and_format_descriptions() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var first = Symbol(); var second = Symbol('desc');\n\
         if (first === second) throw new Error('symbol identity');\n\
         if (String(first) !== 'Symbol()' || String(second) !== 'Symbol(desc)') throw new Error('symbol format');",
    )
    .expect("symbols must preserve identity and descriptions");
}

#[test]
fn regexp_identifier_ascii_class_matches() {
    let mut host = super::RuntimeHost;
    host.run_script(
        r#"if (!/(?:[A-Za-z\xAA\u02C1])/.test('f')) throw new Error('identifier class');"#,
    )
    .expect("ASCII characters must match generated identifier classes");
}

#[test]
fn array_map_call_accepts_array_like_receiver() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var result = Array.prototype.map.call({0: 'a', 1: 'b', length: 2}, String);\n\
         if (result[0] !== 'a' || result[1] !== 'b') throw new Error('map.call');",
    )
    .expect("Array.prototype.map.call must support array-like receivers");
}

#[test]
fn nested_function_return_preserves_false() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function returnsFalse() { return false; }\n\
         function callsIt() { return returnsFalse(); }\n\
         if (callsIt() !== false) throw new Error('false return was lost');",
    )
    .expect("nested function calls must preserve false returns");
}

#[test]
fn loop_early_return_preserves_false() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function differs(a, b) {\n\
           for (var i = 0; i < a.length; i++) {\n\
             if (a[i] !== b[i]) return false;\n\
           }\n\
           return true;\n\
         }\n\
         if (differs([0, 'a'], [0, 'b']) !== false) throw new Error('early return');",
    )
    .expect("loop early returns must preserve false");
}

#[test]
fn loop_body_can_return_false_after_a_matching_iteration() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function differs(a, b) {\n\
           for (var i = 0; i < 2; i++) {\n\
             if (a[i] !== b[i]) return false;\n\
           }\n\
           return true;\n\
         }\n\
         if (differs([0, 'a'], [0, 'b']) !== false) throw new Error('loop return');",
    )
    .expect("loop must propagate a false return after continuing");
}

#[test]
fn counted_loop_updates_index_between_iterations() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var count = 0; for (var i = 0; i < 2; i++) count++;\n\
         if (count !== 2 || i !== 2) throw new Error('loop update');",
    )
    .expect("counted loop must execute and update each iteration");
}

#[test]
fn deleting_a_configurable_property_removes_it() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var obj = {}; Object.defineProperty(obj, 'prop', {value: 42, enumerable: true, configurable: true, writable: true});\n\
         if (!delete obj.prop || Object.prototype.hasOwnProperty.call(obj, 'prop')) throw new Error('delete');",
    )
    .expect("delete must remove configurable own properties");
}

#[test]
fn function_constructor_return_this_matches_script_global() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var fromFunction = Function('return this;')();\n\
         if (fromFunction !== this) throw new Error('global identity');",
    )
    .expect("Function-created code must see the script global object");
}

#[test]
fn constructed_objects_preserve_constructor_identity() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function CustomError(message) { this.message = message; }\n\
         var error = new CustomError('x');\n\
         if (error.constructor !== CustomError) throw new Error('constructor identity');",
    )
    .expect("constructed objects must expose their constructor");
}

#[test]
fn custom_error_prototype_keeps_constructor_identity() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function Test262Error(message) { this.message = message || ''; }\n\
         Test262Error.prototype.toString = function() { return 'Test262Error: ' + this.message; };\n\
         try { throw new Test262Error('x'); } catch (err) {\n\
           if (err.constructor !== Test262Error) throw new Error('error constructor');\n\
         }",
    )
    .expect("custom error instances must retain constructor identity");
}

#[test]
fn dynamic_delete_inside_helper_observes_property_removal() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function isConfigurable(obj, name) { delete obj[name]; return !Object.prototype.hasOwnProperty.call(obj, name); }\n\
         var obj = {}; Object.defineProperty(obj, 'prop', {value: 42, enumerable: true, configurable: true, writable: true});\n\
         if (!isConfigurable(obj, 'prop')) throw new Error('dynamic delete');",
    )
    .expect("helper deletion must observe the updated object state");
}

#[test]
fn bound_primordial_property_helpers_observe_mutation() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "var hasOwn = Function.prototype.call.bind(Object.prototype.hasOwnProperty);\n\
         function isConfigurable(obj, name) { delete obj[name]; return !hasOwn(obj, name); }\n\
         var obj = {}; Object.defineProperty(obj, 'prop', {value: 42, configurable: true});\n\
         if (!isConfigurable(obj, 'prop')) throw new Error('bound delete');",
    )
    .expect("bound primordial helpers must observe property deletion");
}

#[test]
fn indexed_inequality_controls_an_early_return() {
    let mut host = super::RuntimeHost;
    host.run_script(
        "function differs(a, b) {\n\
           if (a[0] !== b[0]) return false;\n\
           return true;\n\
         }\n\
         if (differs(['a'], ['b']) !== false) throw new Error('indexed inequality');",
    )
    .expect("indexed inequality must select the false return");
}

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
    let array = Value::Array(values);
    assert_eq!(quench_runtime::execute::get_property_result(&array, "0").unwrap(), Value::Boolean(true));
    assert!(matches!(quench_runtime::execute::get_property_result(&array, "1"), Ok(Value::Object(_))));
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
fn function_var_is_per_activation() {
    let source = concat!(
        "function f() {\n",
        "  var x;\n",
        "  if (x === undefined) { x = 0; } else { x = 1; }\n",
        "  return x;\n",
        "}\n",
        "void 0;\n",
    );
    let program = quench_runtime::reduce::reduce_source(source).expect("reduce");
    let _ = quench_runtime::vm::execute_code_with_context(program.code(), &quench_runtime::vm::VmContext::isolated())
        .expect("execute");
    let again = quench_runtime::reduce::reduce_source(
        "function f(){ var x; if(x===undefined){x=0;}else{x=1;} return x; } return [f(), f()];",
    )
    .expect("reduce2");
    let value = quench_runtime::vm::execute_code_with_context(again.code(), &quench_runtime::vm::VmContext::isolated())
        .expect("execute2");
    let Value::Array(items) = value else {
        panic!("expected array, got {value:?}");
    };
    let items = Value::Array(items);
    assert_eq!(quench_runtime::execute::get_property_result(&items, "0").unwrap(), Value::Number(0.0), "first call");
    assert_eq!(quench_runtime::execute::get_property_result(&items, "1").unwrap(), Value::Number(0.0), "second call");
}

#[test]
fn module_this_in_method_is_the_instance() {
    let source = concat!(
        "class outer {\n",
        "  #x = 42;\n",
        "  f() { return this.#x; }\n",
        "}\n",
        "export default new outer().f();\n",
    );
    let module = LinkedModule::compile(source).expect("module compiles");
    module.execute().expect("module executes");
    assert_eq!(
        module.export_cell("default").expect("default").get(),
        Value::Number(42.0)
    );
}

#[test]
fn deferred_tla_finishes_before_importer_body() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        concat!(
            "import './setup.js';\n",
            "import defer * as ns from './tla.js';\n",
            "export default globalThis.evaluations.join(',');\n",
        )
        .to_string(),
    );
    graph.add_dependency(
        PathBuf::from("setup.js"),
        "globalThis.evaluations = [];\n".to_string(),
    );
    graph.add_dependency(
        PathBuf::from("tla.js"),
        concat!(
            "globalThis.evaluations.push('tla start');\n",
            "await Promise.resolve(0);\n",
            "globalThis.evaluations.push('tla end');\n",
        )
        .to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    assert_eq!(
        linked.export_cell(entry, "default").expect("default").get(),
        Value::String("tla start,tla end".to_string())
    );
}

#[test]
fn module_throw_statement_fails_execute() {
    let module = LinkedModule::compile("throw { someError: 'x' };").expect("compiles");
    assert!(
        module.execute().is_err(),
        "throw statement must fail LinkedModule::execute"
    );
}

#[test]
fn sequential_harnessed_scripts_do_not_share_bindings() {
    let mut runner = crate::Test262Runner::new(super::RuntimeHost);
    let mut cache = crate::HarnessCache::new(PathBuf::from("tests/test262/harness"));
    let leak = "/*---\nflags: [raw]\n---*/\nvar __quench_batch_leak = 1;\n";
    let probe = concat!(
        "/*---\nflags: [raw]\n---*/\n",
        "if (typeof __quench_batch_leak !== 'undefined') {\n",
        "  throw new Error('leaked');\n",
        "}\n",
    );
    match runner.run_test_with_cache(leak, &mut cache) {
        Ok(crate::TestOutcome::Pass) => {}
        other => panic!("leak script must pass: {other:?}"),
    }
    match runner.run_test_with_cache(probe, &mut cache) {
        Ok(crate::TestOutcome::Pass) => {}
        other => panic!("probe must not see the prior script binding: {other:?}"),
    }
}

#[test]
fn deferred_module_throw_is_replayed_on_get() {
    let mut graph = ModuleGraph::new();
    let entry = graph.add_entry(
        PathBuf::from("entry.js"),
        concat!(
            "import defer * as ns from './throws.js';\n",
            "let caught;\n",
            "try { ns.foo } catch (error) { caught = error }\n",
            "export default caught;\n",
            "export { ns };\n",
        )
        .to_string(),
    );
    graph.add_dependency(
        PathBuf::from("throws.js"),
        "throw { someError: 'x' };\n".to_string(),
    );
    let linked = LinkedModuleGraph::compile(&mut graph).expect("graph compiles");
    linked.execute(&graph, entry).expect("graph executes");
    let ns = linked.export_cell(entry, "ns").expect("ns").get();
    let caught = linked.export_cell(entry, "default").expect("caught").get();
    assert!(
        !matches!(caught, Value::Undefined),
        "evaluation throw must be caught, evaluator={} ns={ns:?} caught={caught:?}",
        quench_runtime::module_bindings::has_evaluator(&ns)
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
