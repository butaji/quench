//! Explicit, expensive executable-link check. Set QUENCH_NATIVE_RUNTIME_RLIB to
//! the aot-enabled rlib reported by cargo build --message-format=json, then run
//! cargo test --profile iteration -p quench-runtime --features aot --test native-image -- --ignored

#![cfg(feature = "aot")]

use cranelift_codegen::{
    cursor::{Cursor, FuncCursor},
    ir::{self, InstBuilder},
};
use quench_runtime::{
    Engine, NativeFunction, NativeJsModule, emit_native_module, lower_native_js_module,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn root_entry_counter(module: &mut NativeJsModule, namespace: &str, image: i64) {
    let index = u32::try_from(module.functions.len()).unwrap();
    let root_name = format!("{namespace}_function_0");
    let root = module
        .functions
        .iter_mut()
        .find_map(|function| match function {
            NativeFunction::Definition { name, function } if *name == root_name => Some(function),
            _ => None,
        })
        .unwrap();
    let mut signature = ir::Signature::new(root.signature.call_conv);
    signature.params.push(ir::AbiParam::new(ir::types::I64));
    let imported = root.import_signature(signature.clone());
    let name = root.declare_imported_user_function(ir::UserExternalName::new(
        quench_runtime::NATIVE_FUNCTION_NAMESPACE,
        index,
    ));
    let callee = root.import_function(ir::ExtFuncData {
        name: ir::ExternalName::user(name),
        signature: imported,
        colocated: false,
        patchable: false,
    });
    let entry = root.layout.entry_block().unwrap();
    let mut cursor = FuncCursor::new(root);
    cursor.goto_first_inst(entry);
    let image = cursor.ins().iconst(ir::types::I64, image);
    cursor.ins().call(callee, &[image]);
    module.functions.push(NativeFunction::Import {
        name: "native_image_entered".into(),
        signature,
    });
}

fn compile_driver(directory: &Path, library: &Path, js: &Path, ts: &Path, output: &Path) -> Output {
    let parent = library.parent().unwrap();
    let dependencies = if parent.join("deps").is_dir() {
        parent.join("deps")
    } else {
        parent.to_owned()
    };
    Command::new("rustc")
        .arg("--edition=2024")
        .arg("--crate-name=native_image_driver")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native-image/driver.rs"))
        .args([
            "-C",
            "opt-level=1",
            "-C",
            "debuginfo=0",
            "-C",
            "codegen-units=256",
        ])
        .arg("-C")
        .arg(format!(
            "incremental={}",
            directory.join("incremental").display()
        ))
        .arg("--extern")
        .arg(format!("quench_runtime={}", library.display()))
        .arg("-L")
        .arg(format!("dependency={}", dependencies.display()))
        .arg("-C")
        .arg(format!("link-arg={}", js.display()))
        .arg("-C")
        .arg(format!("link-arg={}", ts.display()))
        .arg("-o")
        .arg(output)
        .env("QUENCH_NATIVE_JS_METADATA", directory.join("js.qbc"))
        .env("QUENCH_NATIVE_TS_METADATA", directory.join("ts.qbc"))
        .output()
        .expect("compile native image host")
}

#[test]
#[ignore = "compiles a separate Rust host; requires QUENCH_NATIVE_RUNTIME_RLIB"]
fn linked_images_run_without_external_sources_and_reject_an_incompatible_abi() {
    let library = PathBuf::from(
        std::env::var_os("QUENCH_NATIVE_RUNTIME_RLIB")
            .expect("set QUENCH_NATIVE_RUNTIME_RLIB to this build's aot-enabled runtime rlib"),
    );
    assert!(library.is_absolute() && library.is_file());
    let directory = std::env::temp_dir().join(format!(
        "quench-native-image-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let js = Engine::specialize(
        "function add(x) { return x + 1; } function Box(x) { this.answer = add(x); } const box = new Box(41); if (box.answer !== 42 || Object.getPrototypeOf(box) !== Box.prototype) throw 'incorrect construction'; throw box.answer;",
        "native-image.cjs",
    ).unwrap();
    let ts = Engine::specialize_typescript(
        "function add(x: number): number { return x + 1; } const actual: unknown = add('x' as unknown as number); if (actual !== 'x1') throw 'incorrect coercion'; throw 42;",
        "native-image.cts",
    ).unwrap();
    js.write_binary(&directory.join("js.qbc")).unwrap();
    ts.write_binary(&directory.join("ts.qbc")).unwrap();
    let mut js_module = lower_native_js_module(&js, "fixture_js").unwrap();
    let mut ts_module = lower_native_js_module(&ts, "fixture_ts").unwrap();
    assert_eq!(js_module.entry_table_symbol(), "fixture_js_entries");
    assert_eq!(ts_module.entry_table_symbol(), "fixture_ts_entries");
    root_entry_counter(&mut js_module, "fixture_js", 0);
    root_entry_counter(&mut ts_module, "fixture_ts", 1);
    let js_object = directory.join("js.o");
    let ts_object = directory.join("ts.o");
    fs::write(
        &js_object,
        emit_native_module("js", js_module.functions, js_module.data).unwrap(),
    )
    .unwrap();
    fs::write(
        &ts_object,
        emit_native_module("ts", ts_module.functions, ts_module.data).unwrap(),
    )
    .unwrap();
    let executable = directory.join("native-images");
    let link = compile_driver(&directory, &library, &js_object, &ts_object, &executable);
    assert!(
        link.status.success(),
        "{}",
        String::from_utf8_lossy(&link.stderr)
    );
    let isolated = directory.join("isolated");
    fs::create_dir(&isolated).unwrap();
    let isolated_executable = isolated.join("native-images");
    fs::copy(&executable, &isolated_executable).unwrap();
    // No object, metadata or guest source files are present in the run directory.
    let run = Command::new(&isolated_executable)
        .current_dir(&isolated)
        .env_clear()
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"native JS and TS images passed\n");
    assert!(run.stderr.is_empty());

    let mut incompatible = lower_native_js_module(&ts, "fixture_ts").unwrap();
    root_entry_counter(&mut incompatible, "fixture_ts", 1);
    let bad_marker = "quench_native_js_abi_incompatible_fixture";
    for function in &mut incompatible.functions {
        if let NativeFunction::Import { name, .. } = function
            && name == quench_runtime::NATIVE_JS_ABI_SYMBOL
        {
            *name = bad_marker.into();
        }
    }
    let bad_object = directory.join("incompatible.o");
    fs::write(
        &bad_object,
        emit_native_module("incompatible", incompatible.functions, incompatible.data).unwrap(),
    )
    .unwrap();
    let bad_link = compile_driver(
        &directory,
        &library,
        &js_object,
        &bad_object,
        &directory.join("incompatible"),
    );
    assert!(
        !bad_link.status.success(),
        "incompatible runtime ABI unexpectedly linked"
    );
    assert!(
        String::from_utf8_lossy(&bad_link.stderr).contains(bad_marker),
        "{}",
        String::from_utf8_lossy(&bad_link.stderr)
    );
    fs::remove_dir_all(directory).unwrap();
}
