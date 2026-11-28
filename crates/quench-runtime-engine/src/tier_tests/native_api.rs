//! The embedding native API over published images: module graphs, regions and audits (moved from
//! the core's `api_native` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::{Engine, HostCallback, HostFunction, HostFunctionDescriptor, NativeContext, RootedError, Runtime};
use std::cell::RefCell;

#[test]
fn split_host_catalog_preserves_descriptors_and_dispatch_indices() {
    struct SplitHost(Rc<std::cell::Cell<usize>>);
    fn probe(c: &mut NativeContext<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
        Ok(c.number(42.0))
    }
    impl quench_runtime::Host for SplitHost {
        // Its tests exercise no lane behaviour (`Host::runs_lane`).
        fn runs_lane(&self) -> bool { false }
        fn write_line(&mut self, _: &str) { panic!("unexpected output") }
        fn clock_millis(&mut self) -> f64 { 0.0 }
        fn functions(&self) -> &[HostFunction] { panic!("combined catalog must not be read") }
        fn function_count(&self) -> usize { 2 }
        fn function_descriptor(&self, index: usize) -> Option<HostFunctionDescriptor> {
            quench_runtime::host_functions![@descriptors;
                method "hidden" (0) => probe,
                global "catalogProbe" (2) => probe,
            ].get(index).copied()
        }
        fn function_callback(&self, index: usize) -> Option<HostCallback> {
            assert_eq!(index, 1, "metadata reads must not resolve callback addresses");
            self.0.set(self.0.get() + 1);
            Some(probe)
        }
    }
    let source = "if (catalogProbe.name !== 'catalogProbe' || catalogProbe.length !== 2 || typeof hidden !== 'undefined') throw new Error('descriptor'); if (catalogProbe(1, 2) !== 42) throw new Error('dispatch');";
    let compile = Engine::specialize;
    let program = compile(source, "split-host-catalog.js").unwrap();
    let calls = Rc::new(std::cell::Cell::new(0));
    let mut runtime = Runtime::new(SplitHost(calls.clone()));
    runtime.execute(&program).unwrap();
    assert_eq!(calls.get(), 1);
    {
        let native = quench_runtime::NativeJsProgram::publish(program).unwrap();
        let calls = Rc::new(std::cell::Cell::new(0));
        let mut runtime = Runtime::new(SplitHost(calls.clone()));
        runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
        runtime.execute_native(&native).unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(runtime.native_execution_rejections(), 0);
    }
}

#[test]
fn eager_native_module_keeps_harness_closures_alive_after_owner_drop() {
    let mut compiler = quench_runtime::NativeJsCompiler::new();
    let harness = compiler.compile_script(
        "const seed = 7; function readSeed() { return seed; }", "harness.js", Default::default(),
    ).unwrap();
    let entry = compiler.compile_module(
        "globalThis.answer = seed + readSeed() + 28; Promise.resolve().then(() => { answer += 1; });",
        "entry.mjs",
    ).unwrap();
    let entry = compiler.finish(entry).unwrap();
    let residual = entry.clone();
    let harness = quench_runtime::NativeJsProgram::publish(harness).unwrap();
    let native = quench_runtime::NativeJsProgram::publish(entry).unwrap();
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
    runtime.execute_native_after_scripts(&[harness], &native).unwrap();
    drop(native);
    runtime.collect(&residual).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("answer");
    let answer = runtime.get_property_rooted(global, key).unwrap();
    assert_eq!(runtime.rooted_value(answer).unwrap().as_number(), Some(43.0));
    let key = runtime.string_rooted("readSeed");
    let read = runtime.get_property_rooted(global, key).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let result = runtime.call_rooted(read, receiver, &[]).unwrap();
    assert_eq!(runtime.rooted_value(result).unwrap().as_number(), Some(7.0));
}

#[test]
fn native_execution_audit_retains_guest_caught_source_rejections() {
    let program = Engine::specialize(
        "try { eval('1'); } catch (error) { if (!(error instanceof TypeError)) throw error; }", "caught-eval.js",
    ).unwrap();
    let native = quench_runtime::NativeJsProgram::publish(program).unwrap();
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(runtime.native_execution_rejections(), 1);
}

#[test]
fn strict_unresolvable_reference_defers_putvalue_error_until_after_rhs() {
    let source = r#""use strict";
const effects = [];
try { absentReference = (() => { effects.push("rhs"); throw new RangeError("rhs"); })(); throw Error("missing RHS error"); }
catch (error) { if (!(error instanceof RangeError)) throw error; }
try { absentReference = (effects.push("create"), Object.defineProperty(globalThis, "absentReference", { value: 3, configurable: true, writable: true }), 9); throw Error("missing captured reference error"); }
catch (error) { if (!(error instanceof ReferenceError)) throw error; }
if (globalThis.absentReference !== 3) throw Error("RHS property creation was not preserved");
delete globalThis.absentReference;
try { absentReference = (effects.push("plain"), 1); throw Error("missing reference error"); }
catch (error) { if (!(error instanceof ReferenceError)) throw error; }
try { absentReference += (effects.push("compound"), 1); throw Error("missing compound error"); }
catch (error) { if (!(error instanceof ReferenceError)) throw error; }
try { absentReference ||= (effects.push("logical"), 1); throw Error("missing logical error"); }
catch (error) { if (!(error instanceof ReferenceError)) throw error; }
if (effects.join(",") !== "rhs,create,plain") throw Error(effects.join(","));
"#;
    let program = Engine::specialize(source, "strict-reference-order.js").unwrap();
    let modes = [quench_runtime::ExecutionMode::Interpreted, quench_runtime::ExecutionMode::Compiled].into_iter();
    let modes = modes.chain([quench_runtime::ExecutionMode::NativeOnly]);
    for mode in modes {
        let mut runtime = Runtime::new(TestHost::default());
        runtime.set_execution_mode(mode);
        if mode == quench_runtime::ExecutionMode::NativeOnly {
            let native = quench_runtime::NativeJsProgram::publish(program.clone()).unwrap();
            runtime.execute_native(&native).unwrap();
        } else { runtime.execute(&program).unwrap(); }
    }
}

#[test]
fn native_regions_preserve_cross_region_calls_backedges_exceptions_jobs_and_owners() {
    let program = Engine::specialize(r#"
        globalThis.answer = 0;
        function grow(n) {
            let sum = 0;
            for (let i = 0; i < n; i++) {
                try { if (i === 3) throw i; sum += i; }
                catch (error) { sum += error * 2; }
                finally { sum += 1; }
            }
            return () => sum;
        }
        globalThis.saved = grow(8);
        answer = saved();
        Promise.resolve().then(() => { answer += 1; });
    "#, "native-regions.js").unwrap();
    let module = crate::native_object::lower_native_js_module(&program, "native_regions").unwrap();
    assert!(module.functions.iter().filter(|function| matches!(function, crate::native_object::NativeFunction::Definition { .. })).count() > program.function_count());
    crate::native_object::emit_native_module("native_regions", module.functions, module.data).unwrap();
    let native = quench_runtime::NativeJsProgram::publish(program.clone()).unwrap();
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    drop(native);
    runtime.collect(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("answer");
    let answer = runtime.get_property_rooted(global, key).unwrap();
    assert_eq!(runtime.rooted_value(answer).unwrap().as_number(), Some(40.0));
    let key = runtime.string_rooted("saved");
    let saved = runtime.get_property_rooted(global, key).unwrap();
    let receiver = runtime.root(Value::UNDEFINED);
    let value = runtime.call_rooted(saved, receiver, &[]).unwrap();
    assert_eq!(runtime.rooted_value(value).unwrap().as_number(), Some(39.0));
    assert_eq!(runtime.native_execution_rejections(), 0);
}

#[test]
fn native_module_graph_shares_cycles_live_bindings_await_and_namespace_identity() {
    const ENTRY: &str = r#"
        import { count, bump } from './barrel.mjs';
        import * as first from './dep.mjs';
        export function seed() { return 41; }
        if (count !== 41) throw Error('initial live binding');
        bump();
        if (count !== 42) throw Error('updated live binding');
        globalThis.savedModuleClosure = () => count;
        import('./dep.mjs').then(second => {
            if (first !== second || second.count !== 42) throw Error('namespace identity');
            globalThis.moduleJobFinished = true;
        });
    "#;
    const BARREL: &str = "export { count, bump } from './dep.mjs';";
    for awaited in [false, true] {
        let dependency = format!(
            "import {{ seed }} from './entry.mjs'; export let count = {}seed(); export function bump() {{ count++; }}",
            if awaited { "await " } else { "" },
        );
        let sources = [("./entry.mjs", ENTRY), ("./barrel.mjs", BARREL), ("./dep.mjs", dependency.as_str())];
        let mut compiler = quench_runtime::NativeJsCompiler::new();
        let mut programs = sources.iter().map(|(name, source)| {
            let key = quench_runtime::NativeSourceKey::new(name, &[quench_runtime::STATIC_MODULE_GOAL, source]).unwrap();
            compiler.compile_source(&key).unwrap()
        }).collect::<Vec<_>>();
        let entry = compiler.finish(programs.remove(0)).unwrap();
        let entries = Rc::new(RefCell::new(programs.into_iter().zip(&sources[1..]).map(|(program, (name, source))| {
            (name.to_string(), source.to_string(), quench_runtime::NativeJsProgram::publish(program).unwrap())
        }).collect::<Vec<_>>()));
        let lookup = entries.clone();
        let module_sources: Vec<_> = sources.iter().map(|(name, source)| quench_runtime::ModuleSource::text(*name, *source)).collect();
        let host = TestHost::default()
            .with_modules(move |name| Ok(module_sources.iter().find(|module| module.name == name).cloned()))
            .with_native_programs(move |name, parts| Ok(lookup.borrow().iter().find(|(candidate, source, _)| {
                candidate == name && parts == [quench_runtime::STATIC_MODULE_GOAL, source.as_str()]
            }).map(|(_, _, native)| native.clone())));
        let native = quench_runtime::NativeJsProgram::publish(entry.clone()).unwrap();
        let mut runtime = Runtime::new(host);
        runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
        runtime.execute_native(&native).unwrap();
        drop(native);
        entries.borrow_mut().clear();
        runtime.collect(&entry).unwrap();
        let global = runtime.global_root().unwrap();
        let key = runtime.string_rooted("moduleJobFinished");
        let finished = runtime.get_property_rooted(global, key).unwrap();
        assert_eq!(runtime.rooted_value(finished), Some(Value::TRUE));
        let key = runtime.string_rooted("savedModuleClosure");
        let closure = runtime.get_property_rooted(global, key).unwrap();
        let receiver = runtime.root(Value::UNDEFINED);
        let value = runtime.call_rooted(closure, receiver, &[]).unwrap();
        assert_eq!(runtime.rooted_value(value).unwrap().as_number(), Some(42.0));
        assert_eq!(runtime.native_execution_rejections(), 0);
    }
}

#[test]
fn native_module_admission_rejects_wrong_goal_namespace_and_source_before_effects() {
    const SOURCE: &str = "globalThis.dependencyRan = true;";
    for mismatch in ["goal", "namespace", "source"] {
        let mut compiler = quench_runtime::NativeJsCompiler::new();
        let entry = compiler.compile_module("import './dep.mjs'; globalThis.entryRan = true;", "./entry.mjs").unwrap();
        let dependency = if mismatch == "goal" {
            compiler.compile_script(SOURCE, "./dep.mjs", Default::default()).unwrap()
        } else if mismatch == "namespace" {
            quench_runtime::NativeJsCompiler::new().compile_module(SOURCE, "./dep.mjs").unwrap()
        } else {
            compiler.compile_module(SOURCE, "./dep.mjs").unwrap()
        };
        let entry = compiler.finish(entry).unwrap();
        let dependency = quench_runtime::NativeJsProgram::publish(dependency).unwrap();
        let host = TestHost::default()
            .with_modules(move |name| Ok((name == "./dep.mjs").then(|| quench_runtime::ModuleSource::text(name,
                if mismatch == "source" { "globalThis.dependencyRan = false;" } else { SOURCE },
            ))))
            .with_native_programs(move |name, parts| {
                Ok((name == "./dep.mjs" && parts == [quench_runtime::STATIC_MODULE_GOAL, SOURCE])
                    .then(|| dependency.clone()))
            });
        let native = quench_runtime::NativeJsProgram::publish(entry).unwrap();
        let mut runtime = Runtime::new(host);
        runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
        let error = runtime.execute_native(&native).unwrap_err().to_string();
        let expected = match mismatch {
            "goal" => "wrong parse goal",
            "namespace" => "atom IDs do not match",
            _ => "source compilation is unavailable",
        };
        assert!(error.contains(expected), "{mismatch}: {error}");
        let global = runtime.global_root().unwrap();
        for name in ["entryRan", "dependencyRan"] {
            let key = runtime.string_rooted(name);
            let value = runtime.get_property_rooted(global, key).unwrap();
            assert_eq!(runtime.rooted_value(value), Some(Value::UNDEFINED), "{mismatch}: {name}");
        }
        assert_eq!(runtime.native_execution_rejections(), 1);
    }
}

#[test]
fn native_graph_rejects_conflicting_module_tables_before_harness_or_entry_effects() {
    let mut compiler = quench_runtime::NativeJsCompiler::new();
    let harness = compiler.compile_script("globalThis.harnessRan = true;", "harness.js", Default::default()).unwrap();
    let entry = compiler.compile_module("globalThis.entryRan = true;", "entry.mjs").unwrap();
    let dependency = compiler.compile_module("globalThis.depRan = true; export {};", "dep.mjs").unwrap();
    let entry = quench_runtime::NativeJsProgram::publish(compiler.finish(entry).unwrap()).unwrap();
    let dependency = quench_runtime::NativeJsProgram::publish(dependency).unwrap();
    let harness = quench_runtime::NativeJsProgram::publish(harness).unwrap();
    for (modules, message) in [
        (vec![entry.clone()], "duplicates the entry module"),
        (vec![dependency.clone(), dependency], "duplicate native module identity"),
        (vec![harness.clone()], "wrong parse goal"),
    ] {
        let mut runtime = Runtime::new(TestHost::default());
        runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
        let error = runtime.execute_native_module_graph(std::slice::from_ref(&harness), &modules, &entry).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        let global = runtime.global_root().unwrap();
        for name in ["harnessRan", "entryRan", "depRan"] {
            let key = runtime.string_rooted(name);
            let value = runtime.get_property_rooted(global, key).unwrap();
            assert_eq!(runtime.rooted_value(value), Some(Value::UNDEFINED));
        }
    }
}

