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
    link_images(&inputs, &executable, count, None);
    let output = Command::new(&executable).env_clear()
        .current_dir(&directory).output().unwrap();
    assert!(output.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("native Node bootstrap passed: {count} sources\n"));
    fs::remove_dir_all(directory).unwrap();
}

fn link_images(inputs: &quench_node::aot::NativeImageInputs, executable: &Path, count: usize, application: Option<&Path>) {
    use std::{fs, process::Command};
    let node = std::path::PathBuf::from(std::env::var_os("QUENCH_NATIVE_NODE_RLIB").expect("Node rlib"));
    let runtime = std::path::PathBuf::from(std::env::var_os("QUENCH_NATIVE_RUNTIME_RLIB").expect("runtime rlib"));
    let mut command = Command::new("rustc");
    command.args(["--edition=2024", "--crate-name=native_node_bootstrap", "-C", "opt-level=1", "-C", "debuginfo=0", "-C", "codegen-units=256"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aot-bootstrap/driver.rs"))
        .arg("--extern").arg(format!("quench_node={}", node.display()))
        .arg("--extern").arg(format!("quench_runtime={}", runtime.display()))
        .arg("-L").arg(format!("dependency={}", node.parent().unwrap().display()))
        .arg("-o").arg(&executable)
        .env("QUENCH_NATIVE_CATALOG", inputs.catalog())
        .env("QUENCH_NATIVE_SOURCE_COUNT", count.to_string());
    let report = fs::read_to_string(std::env::var_os("QUENCH_NATIVE_BUILD_REPORT").expect("Cargo artifact report")).unwrap();
    for line in report.lines() {
        let artifact: serde_json::Value = serde_json::from_str(line).unwrap();
        if artifact["reason"] == "compiler-artifact" {
            assert!(!matches!(artifact["target"]["name"].as_str(), Some("cranelift_native" | "cranelift_object" | "cranelift_module")), "execution-only support contains a native compiler dependency");
            if artifact["target"]["name"] == "cranelift_codegen" {
                let features = artifact["features"].as_array().expect("Cargo feature list");
                assert!(!features.iter().any(|feature| matches!(feature.as_str(), Some("host-arch" | "all-arch" | "all-native-arch" | "x86" | "arm64" | "riscv64" | "s390x" | "pulley"))), "execution-only support contains an architecture code generator");
            }
        }
        if artifact["reason"] == "compiler-artifact" && matches!(artifact["target"]["name"].as_str(), Some("quench_runtime" | "quench_node")) {
            let features = artifact["features"].as_array().expect("Cargo feature list");
            assert!(features.iter().any(|feature| feature == "native-execution"), "linked support artifact must support native execution");
            assert!(!features.iter().any(|feature| matches!(feature.as_str(), Some("jit" | "aot" | "bundled-sources"))), "linked support artifact must exclude dynamic/object-generation backends and the full SDK source table");
        }
        if artifact["reason"] == "build-script-executed" {
            for path in artifact["linked_paths"].as_array().unwrap() {
                command.arg("-L").arg(path.as_str().unwrap());
            }
        }
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let link_map = {
        let mut path = executable.as_os_str().to_owned();
        path.push(".link-map");
        std::path::PathBuf::from(path)
    };
    #[cfg(target_os = "macos")]
    command.arg("-C").arg(format!("link-arg=-Wl,-map,{}", link_map.display()));
    #[cfg(target_os = "linux")]
    command.arg("-C").arg(format!("link-arg=-Wl,-Map={}", link_map.display()));
    for object in inputs.objects() { command.arg("-C").arg(format!("link-arg={}", object.display())); }
    command.env_remove("QUENCH_NATIVE_APPLICATION");
    if let Some(application) = application { command.env("QUENCH_NATIVE_APPLICATION", application); }
    let link = command.output().unwrap();
    assert!(link.status.success(), "{}", String::from_utf8_lossy(&link.stderr));
    #[cfg(unix)]
    {
        use std::collections::BTreeSet;
        let symbols = Command::new("nm").arg("-g").arg(executable).output().unwrap();
        assert!(symbols.status.success(), "{}", String::from_utf8_lossy(&symbols.stderr));
        let actual: BTreeSet<String> = String::from_utf8(symbols.stdout).unwrap().lines().filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let symbol = fields.last()?.trim_start_matches('_');
            (fields.len() >= 2 && fields[fields.len() - 2] == "T" && symbol.starts_with("quench_js_op_"))
                .then(|| symbol.to_owned())
        }).collect();
        let all_symbols = Command::new("nm").arg(executable).output().unwrap();
        assert!(all_symbols.status.success(), "{}", String::from_utf8_lossy(&all_symbols.stderr));
        let all_symbols = String::from_utf8(all_symbols.stdout).unwrap();
        for producer in ["cranelift_native", "cranelift_object", "cranelift_module", "target_isa_for"] {
            assert!(!all_symbols.lines().any(|line| line.split_whitespace().last().is_some_and(|symbol| symbol.contains(producer))), "linked binary retained native compiler symbol: {producer}");
        }
        let expected: BTreeSet<String> = fs::read_to_string(inputs.required_exports()).unwrap().lines()
            .filter(|symbol| symbol.starts_with("quench_js_op_")).map(str::to_owned).collect();
        assert_eq!(actual, expected, "linked binary retained an unrequested semantic helper export");
    }

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
        let count = manifest.units().len();
        let inputs = manifest.compile().unwrap().emit(&directory.join(format!("{fixture}-images"))).unwrap();
        let executable = directory.join(fixture);
        link_images(&inputs, &executable, count, Some(&entry));
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
