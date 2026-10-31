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
    first.add_module_graph(&entry).unwrap();
    fs::write(&entry, "throw 2;").unwrap();
    let mut second = SourceManifest::default();
    second.add_module_graph(&entry).unwrap();
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
fn host_staging_freezes_lazy_sdk_callbacks_without_running_the_entry() {
    use std::fs;
    let directory = std::env::temp_dir().join(format!("quench-warning-stage-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("entry.cjs");
    fs::write(&entry, "require('node:fs/promises'); require('node:util'); require('node:domain'); throw new Error('application entry must not run');").unwrap();
    let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node");
    let staged = SourceManifest::capture_for_graph(&root, &graph).unwrap();
    let warning = Path::new("lib/internal/process/warning.js");
    assert!(staged.assets().files.keys().any(|path| path.ends_with(warning)));
    assert!(staged.units().any(|unit| Path::new(unit.name()).ends_with(warning)));
    assert!(staged.assets().files.keys().any(|path| path.ends_with("lib/internal/process/report.js")));
    for source in ["lib/internal/fs/promises.js", "lib/internal/abort_controller.js"] {
        assert!(staged.assets().views.keys().any(|(path, _)| path.ends_with(source)), "{source}");
    }
    fs::remove_dir_all(directory).unwrap();
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
    let text = "\u{feff}require('./dep'); require('./data.json'); const load = require; load('unknown'); const x: number = 42; console.error(x); throw x;";
    fs::write(&entry, text).unwrap();
    fs::write(directory.join("dep.js"), "#!/usr/bin/env node\nrequire('./entry.cts'); module.exports = require('node:path');").unwrap();
    fs::write(directory.join("data.json"), "{\"answer\":42}").unwrap();
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_module_graph(&entry).unwrap();
    assert_eq!(graph.files.len(), 3);
    assert_eq!(manifest.units().len(), 2);
    assert_eq!(graph.files[&entry.canonicalize().unwrap()].bytes.as_ref(), text.as_bytes());
    assert!(graph.builtins.contains("node:path"));
    assert!(graph.builtins.contains("node:console"));
    assert!(graph.requirements.iter().any(|edge| matches!(edge, GraphRequirement::DynamicRequire { .. })));
    assert_eq!(manifest.compile().unwrap().units.len(), 2);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn commonjs_hashbang_and_bom_sources_match_node() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench source admission, {}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let sources = ["\u{feff}module.exports=42;", "#!node\nmodule.exports=42;",
        "#!node\rmodule.exports=42;", "#!node\r\nmodule.exports=42;",
        "#!node\u{2028}module.exports=42;", "#!node\u{2029}module.exports=42;",
        "\u{feff}#!node\nmodule.exports=42;"];
    for (index, source) in sources.iter().enumerate() {
        fs::write(directory.join(format!("input{index}.cjs")), source).unwrap();
    }
    let entry = directory.join("runner.cjs");
    fs::write(&entry, format!(r#"
const Module = require('module');
const sources = {sources};
function outcome(action) {{ try {{ return ['ok',action()]; }} catch(error) {{ return [error.name,error.message]; }} }}
console.log(JSON.stringify({{
  compiled: sources.map(source => outcome(() => {{ const module=new Module(); module._compile(source,__filename); return module.exports; }})),
  loaded: sources.map((source,index) => outcome(() => require('./input'+index+'.cjs')))
}}));
"#, sources = serde_json::to_string(&sources).unwrap())).unwrap();
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    for jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_quench-node"));
        if jit { command.arg("--jit"); }
        let actual = command.arg(&entry).output().unwrap();
        assert_eq!(actual.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
        assert_eq!(actual.stdout, oracle.stdout);
        assert_eq!(actual.stderr, oracle.stderr);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn graph_rechecks_a_dependency_created_after_a_failed_resolution() {
    use std::fs;
    let directory = std::env::temp_dir().join(format!("quench-native-later-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("entry.cjs");
    fs::write(&entry, "require('./later');").unwrap();
    let first = SourceManifest::default().add_module_graph(&entry).unwrap();
    assert!(first.requirements.iter().any(|edge| matches!(edge, quench_node::aot::GraphRequirement::Unresolved { .. })));
    fs::write(directory.join("later.js"), "module.exports = 42;").unwrap();
    let second = SourceManifest::default().add_module_graph(&entry).unwrap();
    assert_eq!(second.files.len(), 2);
    assert!(second.requirements.is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn framework_graphs_admit_original_js_and_ts_without_running_applications() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/npm-perf/workloads");
    for fixture in ["react-ssr.cjs", "react-ssr.cts", "next-route-generate.cjs", "next-route-generate.cts"] {
        let mut manifest = SourceManifest::default();
        let graph = manifest.add_module_graph(&root.join(fixture)).unwrap();
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

#[test]
#[ignore = "explicit execution-only SDK admission check"]
fn execution_only_support_rejects_source_and_unlinked_code_in_every_mode() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-sdk-admission-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let program = directory.join("unlinked.qbc");
    quench_runtime::Engine::specialize("print('UNLINKED_EXECUTED');", "unlinked.js")
        .unwrap().write_binary(&program).unwrap();
    let support = quench_node::aot::NativeSupport::from_cargo_report(&pinned_support_report()).unwrap().report();
    let runtime = Path::new(support["runtime_library"].as_str().unwrap());
    let executable = directory.join("sdk-admission");
    let mut compiler = Command::new("rustc");
    compiler.args(["--edition=2024", "-C", "panic=abort", "-C", "debuginfo=0"])
        .arg("-C").arg(format!("opt-level={}", support["optimization_level"].as_str().unwrap()))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aot-bootstrap/execution-only.rs"))
        .arg("--extern").arg(format!("quench_runtime={}", runtime.display()))
        .arg("--extern").arg(format!("quench_node={}", support["node_library"].as_str().unwrap()))
        .arg("-L").arg(format!("dependency={}", runtime.parent().unwrap().display()))
        .arg("-o").arg(&executable);
    if support["rust_lto"] == true { compiler.arg("-C").arg("lto=thin"); }
    for path in support["native_search_paths"].as_array().unwrap() {
        compiler.arg("-L").arg(path.as_str().unwrap());
    }
    let output = compiler.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let output = Command::new(executable).arg(program).env_clear().output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"execution-only support rejects source and unlinked code in every mode\n");
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "explicit execution-only SDK metadata admission check"]
fn bootstrap_images_reject_missing_frozen_sdk_preparation() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-sdk-missing-preparation-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("entry.cjs");
    fs::write(&entry, "require('node:util');").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node");
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_module_graph(&entry).unwrap();
    manifest.extend(SourceManifest::capture_for_graph(&root, &graph).unwrap()).unwrap();
    let mut compiled = manifest.compile().unwrap();
    let (path, file) = compiled.assets.files.iter_mut()
        .find(|(path, _)| path.ends_with("lib/internal/per_context/primordials.js"))
        .expect("bootstrap captures primordials");
    assert!(file.prepared.take().is_some());
    let missing = path.display().to_string();
    let inputs = compiled.emit(&directory.join("images")).unwrap();
    let executable = directory.join("missing-preparation");
    link_images(&inputs, &executable);
    let output = Command::new(executable).env_clear().current_dir(&directory).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&format!("SDK module has no frozen source preparation: {missing}")), "{stderr}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "explicit Wasm JS API native-image qualification"]
fn compile_command_preserves_wasm_validation_and_module_metadata() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench wasm validation, {}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let empty = quench_wasm::Engine::new().compile_wat("(module)").unwrap();
    let shared = quench_wasm::Engine::new().compile_wat_with_features(
        "(module (memory (export \"memory\") 1 2 shared))", quench_wasm::js_api_features(),
    ).unwrap();
    let entry = directory.join("entry.cjs");
    fs::write(&entry, format!(r#"
const empty = new Uint8Array({empty:?});
const shared = new Uint8Array({shared:?});
const storage = new SharedArrayBuffer(empty.byteLength);
new Uint8Array(storage).set(empty);
function outcome(action) {{
  try {{ return action(); }} catch (error) {{ return [error.name, error.message]; }}
}}
async function main() {{
  const module = new WebAssembly.Module(shared);
  const instance = new WebAssembly.Instance(module);
  const memory = instance.exports.memory;
  const before = memory.buffer;
  const view = new Int32Array(before);
  Atomics.store(view, 0, 42);
  const previousPages = memory.grow(1);
  const after = memory.buffer;
  Atomics.add(new Int32Array(after), 0, 1);
  new Uint8Array(after)[65536] = 17;
  new Uint8Array(before, 8, 1).set(new Uint8Array(after, 65536, 1));
  const cloned = structuredClone(before);
  Atomics.add(new Int32Array(cloned), 0, 1);
  const diagnostic = new TypeError('native diagnostic');
  const headline = diagnostic.stack.split('\n')[0];
  diagnostic.stack = headline;
  const result = {{
    validation: [empty, shared, new Uint8Array(), new Uint8Array([1,2,3]), new Uint8Array(storage)]
      .map(bytes => WebAssembly.validate(bytes)),
    rejected: [null, new DataView(empty.buffer), storage]
      .map(value => outcome(() => WebAssembly.validate(value))),
    moduleInput: outcome(() => new WebAssembly.Module(new DataView(empty.buffer))),
    compileInput: await WebAssembly.compile(null).catch(error => [error.name, error.message]),
    emptyCompile: await WebAssembly.compile(new Uint8Array()).catch(error => [error.name, error.message]),
    instantiateInput: await WebAssembly.instantiate(null).catch(error => [error.name, error.message]),
    exports: WebAssembly.Module.exports(module),
    imports: WebAssembly.Module.imports(module),
    memory: [before instanceof SharedArrayBuffer, before.byteLength,
      after instanceof SharedArrayBuffer, after.byteLength, before === after,
      memory.buffer === after, previousPages, view[0], new Int32Array(after)[0],
      new Uint8Array(before)[8], cloned.byteLength, cloned === before],
    rawArrayBuffer: WebAssembly.validate(empty.buffer),
    diagnostic: headline
  }};
  console.log(JSON.stringify(result));
  console.error(diagnostic);
}}
main().catch(error => {{ console.error(error); process.exitCode = 1; }});
"#, empty = empty.bytes(), shared = shared.bytes())).unwrap();
    let sdk = directory.join("support.jsonl");
    fs::write(&sdk, pinned_support_report()).unwrap();
    let binary = directory.join("program");
    let report = directory.join("report.json");
    let compiled = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .arg("compile").arg(&entry).arg("-o").arg(&binary).arg("--report").arg(&report)
        .args(["--profile", "iteration", "--sdk-report"]).arg(sdk).output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    fs::remove_file(entry).unwrap();
    let native = Command::new(&binary).env_clear().current_dir(&directory).output().unwrap();
    assert_eq!(native.status.code(), oracle.status.code(), "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&native.stdout), String::from_utf8_lossy(&native.stderr));
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["bytecode_interpreter"], false);
    assert_eq!(report["js_source_frontend"], false);
    eprintln!("Wasm validation/module metadata matched Node after source removal: {}", binary.display());
}

#[test]
#[ignore = "explicit pooled source identity and reflection qualification"]
fn compile_command_preserves_pooled_source_reflection() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench pooled sources, {}", std::process::id()));
    let sources = directory.join("sources");
    fs::create_dir_all(&sources).unwrap();
    let entry = sources.join("entry.cjs");
    fs::write(&entry, "\u{feff}const fn = require('./dep.cjs');\r\nconsole.log(JSON.stringify({text:fn.toString(),name:fn.name,length:fn.length,value:fn('x'),json:require('./data.json')}));\r\n").unwrap();
    fs::write(sources.join("dep.cjs"), "#!/usr/bin/env node\r\nmodule.exports = function λ(value) { /* 🦀 */\r\n  return value + 'λ';\r\n};\r\n").unwrap();
    fs::write(sources.join("data.json"), "\u{feff}{\"λ\":\"🦀\"}\r\n").unwrap();
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    let sdk = directory.join("support.jsonl");
    fs::write(&sdk, pinned_support_report()).unwrap();
    let binary = directory.join("program");
    let compiled = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .arg("compile").arg(&entry).arg("-o").arg(&binary)
        .args(["--profile", "iteration", "--sdk-report"]).arg(sdk).output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    fs::remove_dir_all(sources).unwrap();
    let native = Command::new(&binary).env_clear().current_dir(&directory).output().unwrap();
    assert_eq!(native.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&native.stderr));
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    eprintln!("Pooled UTF-8, BOM, CRLF, function reflection and JSON matched Node after source removal: {}", binary.display());
}

fn assert_reduced_payload(report: &serde_json::Value, executable: &Path) {
    assert_eq!(report["symbol_audit_stage"], "before_symbol_removal");
    let payload = &report["payload_reduction"];
    let bytes = std::fs::metadata(executable).unwrap().len();
    assert_eq!(report["binary_bytes"], bytes);
    assert_eq!(payload["binary_bytes"], bytes);
    assert!(bytes < payload["unstripped_bytes"].as_u64().unwrap());
    assert!(payload["symbols_after"].as_u64().unwrap() < payload["symbols_before"].as_u64().unwrap());
    assert!(payload["preserved_runtime_sections"].as_u64().unwrap() > 0);
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
        let graph = manifest.add_module_graph(&original).unwrap();
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
        assert_reduced_payload(&report, &executable);
        assert_eq!(report["status"], "compiled");
        assert_eq!(report["execution_mode"], "native_only");
        assert_eq!(report["source_compilation"], false);
        assert_eq!(report["js_source_frontend"], false);
        assert_eq!(report["bytecode_interpreter"], false);
        assert_eq!(report["requested_support_profile"], profile);
        assert_eq!(report["payload_minimal"], false);
        assert!(report["sdk_source_views"].as_u64().unwrap() > 0);
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
        let original_graph = original.add_module_graph(&original_entry).unwrap();
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
        let graph = manifest.add_module_graph(&entry).unwrap();
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

fn write_static_esm_fixture(sources: &Path) -> std::path::PathBuf {
    use std::fs;
    fs::create_dir_all(sources).unwrap();
    let entry = sources.join("entry.mjs");
    fs::write(&entry, r#"
import { count, bump } from './dep.mjs';
import * as fsBare from 'fs';
import * as fsPrefixed from 'node:fs';
import legacy, { answer } from './legacy.cjs';
import data from './data.json' with { type: 'json' };
import * as first from './instance.mjs?tag=/../one';
import * as second from './instance.mjs?tag=/../two';
export function seed() { return 41; }
await Promise.resolve();
bump();
console.log(JSON.stringify({count,answer,legacy:legacy.answer,json:data.answer,
  builtinIdentity:fsBare === fsPrefixed,reader:typeof fsBare.readFile,
  first:first.id,second:second.id,firstUrl:first.url,secondUrl:second.url,
  requireType:typeof require,seedText:seed.toString(),descriptor:Object.getOwnPropertyDescriptor(fsBare, 'readFile').enumerable}));
"#).unwrap();
    fs::write(sources.join("dep.mjs"), "import { seed } from './entry.mjs'; export let count = seed(); export function bump() { count++; }").unwrap();
    fs::write(sources.join("legacy.cjs"), "exports.answer = 42;").unwrap();
    fs::write(sources.join("data.json"), "{\"answer\":43}").unwrap();
    fs::write(sources.join("instance.mjs"), "globalThis.moduleInstances = (globalThis.moduleInstances || 0) + 1; export const id = globalThis.moduleInstances; export const url = import.meta.url;").unwrap();
    entry
}

#[test]
fn static_esm_graph_freezes_node_resolution_facades_and_opaque_url_identities() {
    use std::fs;
    let directory = std::env::temp_dir().join(format!("quench-esm-graph-{}", std::process::id()));
    let entry = write_static_esm_fixture(&directory);
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_module_graph(&entry).unwrap();
    assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
    assert_eq!(graph.files.len(), 5);
    let assets = manifest.assets();
    let root = entry.canonicalize().unwrap().to_string_lossy().into_owned();
    assert_eq!(assets.imports[&(root.clone(), "fs".into())], assets.imports[&(root.clone(), "node:fs".into())]);
    assert_ne!(assets.imports[&(root.clone(), "./instance.mjs?tag=/../one".into())], assets.imports[&(root, "./instance.mjs?tag=/../two".into())]);
    assert_eq!(assets.modules.keys().filter(|name| name.as_str() == "node:quench-builtin/fs").count(), 1);
    manifest.compile().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "explicit source-independent static ESM compile/link qualification"]
fn compile_command_preserves_static_esm_graph_after_source_removal() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench esm sources, {}", std::process::id()));
    let sources = directory.join("sources");
    let entry = write_static_esm_fixture(&sources);
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    let sdk = directory.join("support.jsonl");
    fs::write(&sdk, pinned_support_report()).unwrap();
    let binary = directory.join("program");
    let report = directory.join("report.json");
    let compile = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .arg("compile").arg(&entry).arg("-o").arg(&binary)
        .args(["--profile", "iteration", "--sdk-report"]).arg(&sdk)
        .arg("--report").arg(&report).output().unwrap();
    assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
    fs::remove_dir_all(&sources).unwrap();
    let native = Command::new(&binary).env_clear().current_dir(&directory).output().unwrap();
    assert_eq!(native.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&native.stderr));
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_reduced_payload(&report, &binary);
    assert!(report["archived_module_sources"].as_u64().unwrap() > 0);
    assert!(report["static_import_resolutions"].as_u64().unwrap() > 0);
    assert_eq!(report["bytecode_interpreter"], false);
    assert_eq!(report["js_source_frontend"], false);
    eprintln!("Static ESM cycles/live bindings/TLA/CJS/JSON/builtin aliases/URL identities matched Node after source removal: {}", binary.display());
}

#[test]
fn builtin_esm_facades_bypass_commonjs_cache_poisoning_and_share_alias_identity() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-builtin-facade-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let entry = directory.join("entry.mjs");
    fs::write(&entry, r#"
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
require.cache.fs = { exports: { injected: true } };
const bare = await import('fs');
const prefixed = await import('node:fs');
console.log(JSON.stringify({ identity:bare === prefixed, actual:bare.default === require('node:fs'),
  reader:typeof bare.readFile, poisoned:bare.default.injected === true, cache:require('fs').injected === true }));
"#).unwrap();
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    for jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_quench-node"));
        if jit { command.arg("--jit"); }
        let actual = command.arg(&entry).output().unwrap();
        assert_eq!(actual.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
        assert_eq!(actual.stdout, oracle.stdout);
        assert_eq!(actual.stderr, oracle.stderr);
    }
    fs::remove_dir_all(directory).unwrap();
}

fn write_typescript_esm_fixture(directory: &std::path::Path, extension: &str) -> std::path::PathBuf {
    use std::fs;
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join("package.json"), if extension == "mts" { r#"{"type":"commonjs"}"# } else { r#"{"type":"module"}"# }).unwrap();
    let entry = directory.join(format!("entry.{extension}"));
    fs::write(&entry, r#"
import type { Missing } from './never-present.mts';
export type { MissingAgain } from './also-missing.mts';
export type * from './missing-star.mts';
import { type Shape, value, bump, erased, asserted } from './dep.mts';
import { type Side } from './side.mts';
import * as ns from './dep.mts';
interface Local { value: number }
export interface Exported { value: number }
export type Alias = Local;
export { type Local };
const result: number = erased('text' as unknown as number);
await Promise.resolve();
bump();
console.log(JSON.stringify({result, asserted:asserted(false as unknown as number),
  value, keys:Object.keys(ns), effects:globalThis.typedSideEffects,
  requireType:typeof require, descriptor:Object.getOwnPropertyDescriptor(ns, 'value')}));
"#).unwrap();
    fs::write(directory.join("dep.mts"), r#"
export interface Shape { value: number }
export let value: number = 40;
export function bump(): void { value++; }
export function erased(input: number): number { return input + 1; }
export function asserted(input: unknown): number { return (input as number) + 1; }
"#).unwrap();
    fs::write(directory.join("side.mts"), "export interface Side {}
globalThis.typedSideEffects = (globalThis.typedSideEffects || 0) + 1;
").unwrap();
    entry
}

#[test]
fn typescript_esm_graph_erases_type_edges_without_assuming_value_types() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-ts-esm-graph-{}", std::process::id()));
    for extension in ["mts", "ts"] {
        let root = directory.join(extension);
        let entry = write_typescript_esm_fixture(&root, extension);
        let mut manifest = SourceManifest::default();
        let graph = manifest.add_module_graph(&entry).unwrap();
        assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
        assert_eq!(graph.files.len(), 3);
        assert!(manifest.assets().modules.values().all(|(_, typescript)| *typescript));
        manifest.compile().unwrap();
        let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
        assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
        let output: serde_json::Value = serde_json::from_slice(&oracle.stdout).unwrap();
        assert_eq!(output["result"], "text1");
        assert_eq!(output["asserted"], 1);
        assert_eq!(output["effects"], 1);
        for jit in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_quench-node"));
            if jit { command.arg("--jit"); }
            let actual = command.arg(&entry).output().unwrap();
            assert_eq!(actual.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
            assert_eq!(actual.stdout, oracle.stdout);
            assert_eq!(actual.stderr, oracle.stderr);
        }
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "explicit source-independent TypeScript ESM compile/link qualification"]
fn compile_command_preserves_typescript_esm_after_source_removal() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench ts esm sources, {}", std::process::id()));
    let sources = directory.join("sources");
    let entry = write_typescript_esm_fixture(&sources, "mts");
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    let sdk = directory.join("support.jsonl");
    fs::write(&sdk, pinned_support_report()).unwrap();
    let binary = directory.join("program");
    let report = directory.join("report.json");
    let compile = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .arg("compile").arg(&entry).arg("-o").arg(&binary)
        .args(["--profile", "iteration", "--sdk-report"]).arg(&sdk)
        .arg("--report").arg(&report).output().unwrap();
    assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
    fs::remove_dir_all(&sources).unwrap();
    let native = Command::new(&binary).env_clear().current_dir(&directory).output().unwrap();
    assert_eq!(native.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&native.stderr));
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_reduced_payload(&report, &binary);
    assert_eq!(report["bytecode_interpreter"], false);
    assert_eq!(report["js_source_frontend"], false);
    eprintln!("TypeScript ESM type-only edges and unsound assertions matched Node after source removal: {}", binary.display());
}

#[test]
fn unscoped_typescript_type_import_does_not_force_module_goal() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-ts-unscoped-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let entry = directory.join("entry.ts");
    fs::write(&entry, "import type { Missing } from './missing.mts';\nexport interface Shape {}\nexport type Alias = number;\nexport type * from './missing-star.mts';\nconst value: number = 42; console.log(typeof require, value);\n").unwrap();
    assert_eq!(quench_node::NodeHost::source_kind(&entry).unwrap(), quench_runtime::SourceKind::Script);
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_module_graph(&entry).unwrap();
    assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
    assert_eq!(graph.files.len(), 1);
    manifest.compile().unwrap();
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    for jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_quench-node"));
        if jit { command.arg("--jit"); }
        let actual = command.arg(&entry).output().unwrap();
        assert_eq!(actual.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
        assert_eq!(actual.stdout, oracle.stdout);
        assert_eq!(actual.stderr, oracle.stderr);
    }
    let entry = directory.join("module.ts");
    fs::write(directory.join("side.mts"), "export interface Shape {}\n").unwrap();
    fs::write(&entry, "import { type Shape } from './side.mts'; console.log(typeof require);\n").unwrap();
    assert_eq!(quench_node::NodeHost::source_kind(&entry).unwrap(), quench_runtime::SourceKind::Module);
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_module_graph(&entry).unwrap();
    assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
    assert_eq!(graph.files.len(), 2);
    manifest.compile().unwrap();
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    for jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_quench-node"));
        if jit { command.arg("--jit"); }
        let actual = command.arg(&entry).output().unwrap();
        assert_eq!(actual.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
        assert_eq!(actual.stdout, oracle.stdout);
        assert_eq!(actual.stderr, oracle.stderr);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn typescript_symlink_uses_real_package_scope() {
    use std::{fs, os::unix::fs::symlink, process::Command};
    let directory = std::env::temp_dir().join(format!("quench-ts-symlink-{}", std::process::id()));
    let scoped = directory.join("scoped");
    fs::create_dir_all(&scoped).unwrap();
    fs::write(scoped.join("package.json"), r#"{"type":"module"}"#).unwrap();
    let original = scoped.join("entry.ts");
    fs::write(&original, "const value: number = 42; console.log(typeof require, value);\n").unwrap();
    let entry = directory.join("linked.ts");
    symlink(&original, &entry).unwrap();
    assert_eq!(quench_node::NodeHost::source_kind(&entry).unwrap(), quench_runtime::SourceKind::Module);
    let oracle = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    for jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_quench-node"));
        if jit { command.arg("--jit"); }
        let actual = command.arg(&entry).output().unwrap();
        assert_eq!(actual.status.code(), oracle.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
        assert_eq!(actual.stdout, oracle.stdout);
        assert_eq!(actual.stderr, oracle.stderr);
    }
    fs::remove_dir_all(directory).unwrap();
}
