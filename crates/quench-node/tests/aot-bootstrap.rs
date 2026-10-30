#![cfg(feature = "aot")]

use quench_node::aot::SourceManifest;
use quench_runtime::{HostScriptOptions, NativeJsCompiler, NativeSourceKey, emit_native_module, lower_native_js_module};
use std::path::Path;

#[test]
fn frozen_module_inputs_reject_conflicting_entries_without_changing_the_manifest() {
    use std::fs;
    let directory = std::env::temp_dir().join(format!("quench-assets-conflict-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("entry.cjs");
    fs::write(&entry, "throw 1;").unwrap();
    let mut first = SourceManifest::default();
    first.add_commonjs_graph(&entry).unwrap();
    fs::write(&entry, "throw 2;").unwrap();
    let mut second = SourceManifest::default();
    second.add_commonjs_graph(&entry).unwrap();
    assert!(first.extend(second).unwrap_err().contains("conflicting module input"));
    assert_eq!(first.assets().files[&entry.canonicalize().unwrap()].bytes.as_ref(), b"throw 1;");
    assert_eq!(first.units().len(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn source_keys_reject_unknown_or_noncanonical_shapes() {
    for parts in [
        vec!["specialized", "script", ""],
        vec!["specialized", "specialized-script:01:0:false", ""],
        vec!["specialized", "specialized-script:0:0:yes", ""],
        vec!["specialized", "specialized-script:0:0:false:jsx", ""],
        vec!["other-mode", ""],
    ] {
        assert!(NativeSourceKey::new("unit", &parts).is_err(), "{parts:?}");
    }
    let key = NativeSourceKey::new("unit", &["unspecialized", ""]).unwrap();
    assert!(NativeJsCompiler::new(true).compile_source(&key).is_err());
}

#[test]
fn manifest_retains_full_source_identity_and_compiles_without_guest_execution() {
    let mut manifest = SourceManifest::default();
    let options = HostScriptOptions { typescript: true, ..Default::default() };
    let goal = options.cache_goal();
    let parts = ["specialized", &goal, "throw ('x' as unknown as number) + 1;"];
    assert!(manifest.insert("unit.cts", &parts).unwrap());
    assert!(!manifest.insert("unit.cts", &parts).unwrap());
    assert!(manifest.insert("other.cts", &parts).unwrap());
    assert!(manifest.insert("unit.cts", &["specialized", &goal, "throw 42;"]).unwrap());
    assert_eq!(manifest.units().len(), 3);
    let compiled = manifest.compile().unwrap();
    assert_eq!(compiled.units.len(), 3);
    for (index, unit) in compiled.units.iter().enumerate() {
        let module = lower_native_js_module(&unit.program, &format!("unit_{index}")).unwrap();
        assert!(!emit_native_module("unit", module.functions, module.data).unwrap().is_empty());
    }
}

#[test]
fn commonjs_graph_keeps_cycles_json_dynamic_edges_and_original_bytes() {
    use quench_node::aot::GraphRequirement;
    use std::fs;
    let directory = std::env::temp_dir().join(format!("quench-native-graph-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("entry.cts");
    let text = "\u{feff}#!/usr/bin/env node\nrequire('./dep'); require('./data.json'); const load = require; load('unknown'); const x: number = 42; throw x;";
    fs::write(&entry, text).unwrap();
    fs::write(directory.join("dep.js"), "require('./entry.cts'); module.exports = require('node:path');").unwrap();
    fs::write(directory.join("data.json"), "{\"answer\":42}").unwrap();
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_commonjs_graph(&entry).unwrap();
    assert_eq!(graph.files.len(), 3);
    assert_eq!(manifest.units().len(), 2);
    assert_eq!(graph.files[&entry.canonicalize().unwrap()].bytes.as_ref(), text.as_bytes());
    assert!(graph.builtins.contains("node:path"));
    assert!(graph.requirements.iter().any(|edge| matches!(edge, GraphRequirement::DynamicRequire { .. })));
    assert_eq!(manifest.compile().unwrap().units.len(), 2);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn graph_rechecks_a_dependency_created_after_a_failed_resolution() {
    use std::fs;
    let directory = std::env::temp_dir().join(format!("quench-native-later-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("entry.cjs");
    fs::write(&entry, "require('./later');").unwrap();
    let first = SourceManifest::default().add_commonjs_graph(&entry).unwrap();
    assert!(first.requirements.iter().any(|edge| matches!(edge, quench_node::aot::GraphRequirement::Unresolved { .. })));
    fs::write(directory.join("later.js"), "module.exports = 42;").unwrap();
    let second = SourceManifest::default().add_commonjs_graph(&entry).unwrap();
    assert_eq!(second.files.len(), 2);
    assert!(second.requirements.is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn framework_graphs_admit_original_js_and_ts_without_running_applications() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/npm-perf/workloads");
    for fixture in ["react-ssr.cjs", "react-ssr.cts", "next-route-generate.cjs", "next-route-generate.cts"] {
        let mut manifest = SourceManifest::default();
        let graph = manifest.add_commonjs_graph(&root.join(fixture)).unwrap();
        eprintln!("{fixture}: {} files, {} source units, {} builtins, {} requirements", graph.files.len(), manifest.units().len(), graph.builtins.len(), graph.requirements.len());
        for requirement in &graph.requirements { eprintln!("  {requirement:?}"); }
        let count = manifest.units().len();
        let compiled = manifest.compile().unwrap();
        assert_eq!(compiled.units.len(), count);
        assert!(compiled.units.len() > 1);
    }
}

#[test]
fn node_bootstrap_manifest_compiles_to_native_objects_before_application_entry() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node");
    let manifest = SourceManifest::capture_bootstrap(&root).unwrap();
    assert!(manifest.units().len() > 0, "bootstrap bypassed the source admission boundary");
    let count = manifest.units().len();
    let compiled = manifest.compile().unwrap();
    assert_eq!(compiled.units.len(), count);
    let mut object_bytes = 0;
    let mut functions = 0;
    for (index, unit) in compiled.units.iter().enumerate() {
        functions += unit.program.function_count();
        let module = lower_native_js_module(&unit.program, &format!("bootstrap_{index}")).unwrap();
        object_bytes += emit_native_module("bootstrap", module.functions, module.data).unwrap().len();
    }
    let module = lower_native_js_module(&compiled.entry, "entry").unwrap();
    object_bytes += emit_native_module("entry", module.functions, module.data).unwrap().len();
    eprintln!("bootstrap: {count} source units, {functions} functions, {object_bytes} object bytes");
    assert!(object_bytes > 0);
}

/// Set both rlib paths from Cargo's exact execution-only build artifacts.
#[test]
#[ignore = "explicit native executable link check"]
fn bootstrap_images_link_and_execute_in_node_host() {
    use std::{fs, process::Command};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node").canonicalize().unwrap();
    let manifest = SourceManifest::capture_bootstrap(&root).unwrap();
    let count = manifest.units().len();
    let directory = std::env::temp_dir().join(format!("quench-native-node-bootstrap-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let inputs = manifest.compile().unwrap().emit(&directory.join("images")).unwrap();
    let executable = directory.join("native-node-bootstrap");
    link_images(&inputs, &executable);
    let output = Command::new(&executable).env_clear()
        .current_dir(&directory).output().unwrap();
    assert!(output.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("native Node bootstrap passed: {count} sources\n"));
    fs::remove_dir_all(directory).unwrap();
}

fn pinned_support_report() -> String {
    use std::fs;
    let report = fs::read_to_string(std::env::var_os("QUENCH_NATIVE_BUILD_REPORT").expect("Cargo artifact report")).unwrap();
    // The producer rebuild can replace Cargo's top-level copy after the SDK
    // report was captured. The harness pins its exact dependency artifacts.
    report.lines().map(|line| {
        let mut artifact: serde_json::Value = serde_json::from_str(line).unwrap();
        if artifact["reason"] == "compiler-artifact" {
            let variable = match artifact["target"]["name"].as_str() {
                Some("quench_node") => Some("QUENCH_NATIVE_NODE_RLIB"),
                Some("quench_runtime") => Some("QUENCH_NATIVE_RUNTIME_RLIB"),
                _ => None,
            };
            if let Some(variable) = variable {
                artifact["filenames"] = serde_json::json!([std::env::var(variable).unwrap()]);
            }
        }
        serde_json::to_string(&artifact).unwrap()
    }).collect::<Vec<_>>().join("\n")
}

fn link_images(inputs: &quench_node::aot::NativeImageInputs, executable: &Path) {
    let support = quench_node::aot::NativeSupport::from_cargo_report(&pinned_support_report()).unwrap();
    support.link(inputs, &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aot-bootstrap/driver.rs"), executable).unwrap();
}

#[test]
#[ignore = "explicit compile-command framework qualification"]
fn compile_command_builds_source_independent_framework_js_and_ts() {
    qualify_compile_command_frameworks("iteration", Some(pinned_support_report()));
}

#[test]
#[ignore = "explicit default production framework compile/link qualification"]
fn production_compile_command_builds_source_independent_framework_js_and_ts() {
    qualify_compile_command_frameworks("production", None);
}

fn qualify_compile_command_frameworks(profile: &str, support: Option<String>) {
    use std::{fs, process::Command};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/npm-perf/workloads");
    let app_root = fixtures.parent().unwrap().canonicalize().unwrap();
    let node = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).args(["-p", "process.execPath"]).output().unwrap();
    assert!(node.status.success());
    let node = String::from_utf8(node.stdout).unwrap().trim().to_owned();
    // Exercise spaces and commas through rustc and the platform linker.
    let directory = std::env::temp_dir().join(format!("quench cli, frameworks-{profile}-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let sdk_report = directory.join("support.jsonl");
    if let Some(support) = &support { fs::write(&sdk_report, support).unwrap(); }
    for fixture in ["react-ssr.cjs", "react-ssr.cts", "next-route-generate.cjs", "next-route-generate.cts"] {
        let original = fixtures.join(fixture).canonicalize().unwrap();
        let mut manifest = SourceManifest::default();
        let graph = manifest.add_commonjs_graph(&original).unwrap();
        let sources = directory.join(format!("{fixture}-sources"));
        copy_graph_sources(&graph, &app_root, &sources);
        let entry = sources.join(original.strip_prefix(&app_root).unwrap());
        let executable = directory.join(fixture);
        let report = directory.join(format!("{fixture}.json"));
        let mut compiler = Command::new(env!("CARGO_BIN_EXE_quench-node"));
        compiler.arg("compile").arg(&entry).arg("-o").arg(&executable).arg("--report").arg(&report)
            .env_remove(quench_node::shared_run::NODE_LIBRARY_ROOT_ENV);
        if support.is_some() { compiler.args(["--profile", profile]).arg("--sdk-report").arg(&sdk_report); }
        let compiled = compiler.output().unwrap();
        assert!(compiled.status.success(), "compile {fixture}: {}", String::from_utf8_lossy(&compiled.stderr));
        let run = |command: &mut Command| {
            let result = command.env_clear().env("NODE_ENV", "production")
                .env("QUENCH_BENCH_ROUNDS", "20").env("QUENCH_BENCH_WARMUPS", "5")
                .current_dir(&directory).output().unwrap();
            assert!(result.status.success(), "{fixture}: {}", String::from_utf8_lossy(&result.stderr));
            assert!(result.stderr.is_empty(), "{fixture}: {}", String::from_utf8_lossy(&result.stderr));
            let mut value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            value.as_object_mut().unwrap().remove("elapsedMs");
            value.as_object_mut().unwrap().remove("operationsPerSecond");
            value
        };
        let oracle = run(Command::new(&node).arg(&entry));
        fs::remove_dir_all(&sources).unwrap();
        assert_eq!(run(&mut Command::new(&executable)), oracle);
        let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["status"], "compiled");
        assert_eq!(report["execution_mode"], "native_only");
        assert_eq!(report["requested_support_profile"], profile);
        assert_eq!(report["payload_minimal"], false);
        assert!(!report["retained_support"].as_array().unwrap().is_empty());
        eprintln!("compile command {fixture}: Node parity after source removal; executable={}", executable.display());
    }
}

#[test]
#[ignore = "explicit default production compile/link qualification"]
fn production_compile_preserves_typescript_values_arguments_and_exit_lifecycle() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-cli-production-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("sound.cts");
    fs::write(&entry, r#"
function add(value: number): number { return value + 1; }
const input = "7" as unknown as number;
console.log(JSON.stringify([typeof input, add(input), Number.isNaN(input), process.argv.slice(2)]));
process.on("exit", (code: number) => console.log("exit", code));
process.exitCode = 7;
"#).unwrap();
    let executable = directory.join("program");
    let report = directory.join("report.json");
    let compiled = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .arg("compile").arg(&entry).arg("-o").arg(&executable)
        .arg("--report").arg(&report)
        .env_remove(quench_node::shared_run::NODE_LIBRARY_ROOT_ENV).output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into()))
        .arg(&entry).arg("guest value, spaces").output().unwrap();
    fs::remove_file(entry).unwrap();
    let native = Command::new(&executable).arg("guest value, spaces").env_clear().current_dir(&directory).output().unwrap();
    assert_eq!(native.status.code(), oracle.status.code());
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["requested_support_profile"], "production");
    assert_eq!(report["support_artifacts"]["rust_lto"], true);
    assert_eq!(report["payload_minimal"], false);
    eprintln!("production compile: sound TS values, argv and exit lifecycle matched Node; executable={}", executable.display());
}

#[test]
#[ignore = "explicit framework native executable link checks"]
fn framework_images_execute_original_js_and_ts_and_match_node() {
    use std::{fs, process::Command};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node").canonicalize().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/npm-perf/workloads");
    let node = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into()))
        .args(["-p", "process.execPath"]).output().unwrap();
    assert!(node.status.success());
    let node = String::from_utf8(node.stdout).unwrap().trim().to_owned();
    let directory = std::env::temp_dir().join(format!("quench-native-frameworks-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    for fixture in ["react-ssr.cjs", "react-ssr.cts", "next-route-generate.cjs", "next-route-generate.cts"] {
        let original_entry = fixtures.join(fixture).canonicalize().unwrap();
        let mut original = SourceManifest::default();
        let original_graph = original.add_commonjs_graph(&original_entry).unwrap();
        let sdk = SourceManifest::capture_for_graph(&root, &original_graph).unwrap();
        let source_directory = directory.join(format!("{fixture}-sources"));
        let app_root = source_directory.join("app");
        let sdk_root = source_directory.join("sdk");
        fs::create_dir_all(&sdk_root).unwrap();
        let original_app_root = fixtures.parent().unwrap().canonicalize().unwrap();
        copy_graph_sources(&original_graph, &original_app_root, &app_root);
        for (path, file) in &sdk.assets().files {
            let copy = sdk_root.join(path.strip_prefix(&root).expect("SDK staging loaded an external application module"));
            fs::create_dir_all(copy.parent().unwrap()).unwrap();
            fs::write(copy, file.bytes.as_ref()).unwrap();
        }
        let entry = app_root.join(original_entry.strip_prefix(&original_app_root).unwrap()).canonicalize().unwrap();
        let mut manifest = SourceManifest::default();
        let graph = manifest.add_commonjs_graph(&entry).unwrap();
        manifest.extend(SourceManifest::capture_for_graph(&sdk_root, &graph).unwrap()).unwrap();
        eprintln!("native {fixture}: {} files, {} source units, {} requirements", graph.files.len(), manifest.units().len(), graph.requirements.len());
        let inputs = manifest.compile().unwrap().emit(&directory.join(format!("{fixture}-images"))).unwrap();
        let executable = directory.join(fixture);
        link_images(&inputs, &executable);
        let run = |command: &mut Command| {
            let output = command.env_clear()
                .env("NODE_ENV", "production").env("QUENCH_BENCH_ROUNDS", "20").env("QUENCH_BENCH_WARMUPS", "5")
                .current_dir(&directory).output().unwrap();
            assert!(output.status.success(), "{fixture}: stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            let mut json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            json.as_object_mut().unwrap().remove("elapsedMs");
            json.as_object_mut().unwrap().remove("operationsPerSecond");
            json
        };
        let oracle = run(Command::new(&node).arg(&entry));
        fs::remove_dir_all(&source_directory).unwrap();
        assert!(!entry.exists() && !sdk_root.exists());
        let native = run(&mut Command::new(&executable));
        assert_eq!(native, oracle, "{fixture}");
        eprintln!("native {fixture}: Node parity passed; executable={}", executable.display());
    }
}

fn copy_graph_sources(graph: &quench_node::aot::DependencyGraph, original_root: &Path, target_root: &Path) {
    use std::{collections::BTreeSet, fs};
    let mut packages = BTreeSet::new();
    for (path, file) in &graph.files {
        let target = target_root.join(path.strip_prefix(original_root).unwrap());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, file.bytes.as_ref()).unwrap();
        let mut directory = path.parent();
        while let Some(parent) = directory.filter(|parent| parent.starts_with(original_root)) {
            let package = parent.join("package.json");
            if package.is_file() { packages.insert(package); }
            directory = parent.parent();
        }
    }
    for package in packages {
        let target = target_root.join(package.strip_prefix(original_root).unwrap());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(package, target).unwrap();
    }
}
