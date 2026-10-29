//! Writes a Wasm-ESM training corpus: the spec testsuite's modules as `.wasm` files that
//! `run.mjs` imports through Node's Wasm ESM integration, invoking each module's actions.
//! It trains the static `quench-node` layout on the Wasm load path (resolution, validation,
//! lowering, instantiation, the facade and its warning, imports from a JavaScript package) and
//! the interpreter, from generic programs rather than a scored benchmark.
//!
//! ```sh
//! wasm-esm-corpus OUT_DIR WAST_FILE...
//! quench-node OUT_DIR/run.mjs [returns]
//! ```
//!
//! A module is kept when everything it imports comes from `spectest`, which
//! `OUT_DIR/node_modules/spectest` provides. Actions are the invocations of the latest unnamed
//! module with numeric arguments; each records whether the script expects it to return.

use std::{env, fs, path::PathBuf, process::ExitCode};

use serde_json::{json, Value};

/// The driver and the spec host package, written beside the modules.
const DRIVER: &str = include_str!("../../esm-corpus/run.mjs");
const SPECTEST: &str = include_str!("../../esm-corpus/spectest.js");
const SPECTEST_PACKAGE: &str =
    r#"{ "name": "spectest", "type": "module", "exports": "./index.js" }"#;

/// The only import module a kept module may name.
const HOST_MODULE: &str = "spectest";

/// Argument types `run.mjs` converts from their bits.
const NUMERIC_TYPES: &[&str] = &["i32", "i64", "f32", "f64"];

/// Commands whose action the script expects to return.
const RETURNING_COMMANDS: &[&str] = &["assert_return", "action"];

/// Whether every import of `binary` comes from `HOST_MODULE`.
fn imports_only_host(binary: &[u8]) -> bool {
    quench_wasm::interface(binary)
        .is_some_and(|interface| interface.imports.iter().all(|import| import.module == HOST_MODULE))
}

/// Decodes a lowercase hexadecimal module binary.
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// The action of `command` as `[field, [[type, bits]...], returns]`, when it invokes the
/// current module with numeric arguments.
fn action(command: &Value) -> Option<Value> {
    let action = command.get("action")?;
    if action["type"] != "invoke" || !action["module"].is_null() {
        return None;
    }
    let args = action["args"]
        .as_array()?
        .iter()
        .map(|arg| {
            let ty = arg["type"].as_str().filter(|ty| NUMERIC_TYPES.contains(ty))?;
            Some(json!([ty, arg["bits"]]))
        })
        .collect::<Option<Vec<_>>>()?;
    let returns = command["cmd"].as_str().is_some_and(|cmd| RETURNING_COMMANDS.contains(&cmd));
    Some(json!([action["field"], args, returns]))
}

fn run(out: &PathBuf, files: &[PathBuf]) -> Result<(), String> {
    let write = |path: PathBuf, contents: &[u8]| {
        fs::create_dir_all(path.parent().expect("output has a parent"))
            .and_then(|()| fs::write(&path, contents))
            .map_err(|error| format!("{}: {error}", path.display()))
    };
    let mut modules: Vec<Value> = Vec::new();
    for file in files {
        let source = fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;
        let script = quench_wasm_test::wast_to_json(&file.to_string_lossy(), &source)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        // The module later actions invoke, or None after one the corpus skips.
        let mut current: Option<usize> = None;
        for command in script["commands"].as_array().into_iter().flatten() {
            if command["cmd"] == "module" {
                current = None;
                let Some(binary) = command["binary"].as_str().and_then(decode_hex) else { continue };
                if !imports_only_host(&binary) {
                    continue;
                }
                let name = format!("m{}.wasm", modules.len());
                write(out.join(&name), &binary)?;
                current = Some(modules.len());
                modules.push(json!({ "file": name, "actions": [] }));
            } else if let (Some(index), Some(action)) = (current, action(command)) {
                modules[index]["actions"].as_array_mut().expect("actions is an array").push(action);
            }
        }
    }
    write(out.join("corpus.json"), Value::Array(modules).to_string().as_bytes())?;
    write(out.join("run.mjs"), DRIVER.as_bytes())?;
    let package = out.join("node_modules").join(HOST_MODULE);
    write(package.join("package.json"), SPECTEST_PACKAGE.as_bytes())?;
    write(package.join("index.js"), SPECTEST.as_bytes())
}

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1).map(PathBuf::from);
    let Some(out) = arguments.next() else {
        eprintln!("usage: wasm-esm-corpus OUT_DIR WAST_FILE...");
        return ExitCode::from(2);
    };
    let files: Vec<PathBuf> = arguments.collect();
    match run(&out, &files) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
