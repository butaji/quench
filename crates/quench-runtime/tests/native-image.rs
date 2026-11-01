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

fn entry_counter(module: &mut NativeJsModule, image: i64, export: &str, selects: impl Fn(&str) -> bool) {
    let index = u32::try_from(module.functions.len()).unwrap();
    let mut signature = None;
    for function in &mut module.functions {
        let NativeFunction::Definition { name, function } = function else { continue; };
        if !selects(name) { continue; }
        let mut counter = ir::Signature::new(function.signature.call_conv);
        counter.params.push(ir::AbiParam::new(ir::types::I64));
        signature.get_or_insert_with(|| counter.clone());
        let imported = function.import_signature(counter);
        let name = function.declare_imported_user_function(ir::UserExternalName::new(
            quench_runtime::NATIVE_FUNCTION_NAMESPACE, index,
        ));
        let callee = function.import_function(ir::ExtFuncData {
            name: ir::ExternalName::user(name), signature: imported, colocated: false, patchable: false,
        });
        let entry = function.layout.entry_block().unwrap();
        let mut cursor = FuncCursor::new(function);
        cursor.goto_first_inst(entry);
        let image = cursor.ins().iconst(ir::types::I64, image);
        cursor.ins().call(callee, &[image]);
    }
    module.functions.push(NativeFunction::Import {
        name: export.into(), signature: signature.expect("fixture entry was not emitted"),
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
    let scalar_fixture = include_str!("fixtures/scalar-regions.js");
    let store_fixture = format!("{}\n{scalar_fixture}", include_str!("fixtures/captured-field-store.js"));
    let js = Engine::specialize(
        &format!("{store_fixture}{}", "function makeGuard(mask, expected) { function check() { return (this.value & mask) !== 0 || this.value == expected; } return {check, setMask(value) { mask = value; }}; } function exerciseGuard() { const guard = makeGuard(2, 4); guard.value = 4; for (let n = 0; n < 64; n++) if (!guard.check()) throw 'boolean direct'; const original = guard.check; guard.check = function() { return false; }; if (guard.check()) throw 'boolean replacement fallback'; guard.check = original; if (!guard.check(99)) throw 'boolean arguments fallback'; function tail(g) { return g.check(); } if (!tail(guard)) throw 'boolean tail fallback'; guard.setMask(4294967297); guard.value = 3; if (!guard.check()) throw 'boolean int32 fallback'; guard.setMask(1.5); guard.value = 3; if (!guard.check()) throw 'boolean fraction fallback'; guard.setMask(1n); let mixedThrew = false; try { guard.check(); } catch (error) { mixedThrew = error instanceof TypeError; } if (!mixedThrew) throw 'boolean bigint fallback'; guard.setMask(2); let reads = 0; Object.defineProperty(guard, 'value', { configurable: true, get() { reads++; return reads === 1 ? 0 : 4; } }); if (!guard.check() || reads !== 2) throw 'boolean getter fallback'; } exerciseGuard(); function add(x) { return x + 1; } function Box(x) { this.a = 1; this.b = 2; this.answer = add(x); } Box.prototype.read = function () { return this.answer; }; const box = new Box(41); if (Object.getPrototypeOf(box) !== Box.prototype) throw 'incorrect construction'; for (let n = 0; n < 16; n++) if (box.read() !== 42) throw 'own field'; const small = { answer: -7, read: Box.prototype.read }; for (let n = 0; n < 16; n++) if (small.read() !== -7) throw 'inline field'; const reference = {}; for (const value of [0, true, false, null, undefined, reference]) { small.answer = value; if (small.read() !== value) throw 'inline value'; } small.answer = 1.5; if (small.read() !== 1.5) throw 'wide field'; Object.defineProperty(box, 'answer', { get() { return 43; } }); if (box.read() !== 43) throw 'field accessor fallback'; function inheritedRead(object) { const value = object.value; if (value === null) return null; return value; } const prototype = { value: 70, a: 1, b: 2 }; const child = Object.assign(Object.create(prototype), { a: 3, b: 4, c: 5 }); for (let n = 0; n < 32; n++) if (inheritedRead(child) !== 70) throw 'prototype cache'; prototype.value = 71; if (inheritedRead(child) !== 71) throw 'prototype live value'; child.value = 72; if (inheritedRead(child) !== 72) throw 'own shadow'; delete child.value; if (inheritedRead(child) !== 71) throw 'remove shadow'; Object.defineProperty(prototype, 'value', { get() { return 73; }, configurable: true }); if (inheritedRead(child) !== 73) throw 'prototype accessor'; Object.setPrototypeOf(child, { value: 74 }); if (inheritedRead(child) !== 74) throw 'prototype replacement'; function captureOuter() { let value = { n: 1 }; function read() { let result; for (let n = 0; n < 32; n++) result = value; return result; } if (read() !== value) throw 'capture identity'; value = { n: 2 }; if (read().n !== 2) throw 'capture mutation'; } captureOuter(); function looseNull(x) { if (x == null) return 1; return 0; } function strictNull(x) { if (x === null) return 1; return 0; } for (const x of [null, undefined, 0, false, {}, []]) { if (looseNull(x) !== (x === null || x === undefined ? 1 : 0)) throw 'loose null'; if (strictNull(x) !== (x === null ? 1 : 0)) throw 'strict null'; } function writeThis(value) { this.x = value; } const receiver = { a: 1, b: 2, c: 3, x: 0, writeThis }; for (let n = 0; n < 32; n++) receiver.writeThis(n); if (receiver.x !== 31) throw 'this store'; const childValue = { n: 8 }; receiver.writeThis(childValue); if (receiver.x !== childValue) throw 'this heap store'; let assigned = 0; Object.defineProperty(receiver, 'x', { set(value) { assigned = value; }, configurable: true }); receiver.writeThis(42); if (assigned !== 42) throw 'this setter fallback'; function readThis() { 'use strict'; const receiver = this; if (receiver === null) return 1; return receiver; } const readOwner = { readThis }; for (let n = 0; n < 32; n++) if (readOwner.readThis() !== readOwner) throw 'this load'; if (readThis.call(null) !== 1 || readThis.call(undefined) !== undefined) throw 'strict this'; function fusedCopy() { this.out = this.holder.value; return this.out; } const copyHolder = { a: 1, b: 2, value: 7 }; const copyTarget = { a: 1, b: 2, c: 3, holder: copyHolder, out: 0, fusedCopy }; for (let n = 0; n < 32; n++) if (copyTarget.fusedCopy() !== 7) throw 'fused copy'; let copyReads = 0; Object.defineProperty(copyHolder, 'value', { get() { copyReads++; return 8; }, configurable: true }); if (copyTarget.fusedCopy() !== 8 || copyReads !== 1) throw 'fused getter fallback'; function equalReturn(a, b) { return a == b; } function strictReturn(a, b) { return a === b; } for (let n = 0; n < 32; n++) if (!equalReturn(n, n) || !strictReturn(n, n)) throw 'return relation'; if (!strictReturn(0, -0) || strictReturn(NaN, NaN) || !equalReturn(4n, 4n)) throw 'return relation fallback'; class Base {} class Derived extends Base { constructor() { let caught = false; try { this.x; } catch (e) { caught = e instanceof ReferenceError; } if (!caught) throw 'uninitialized this'; super(); this.x = 2; } } if (new Derived().x !== 2) throw 'initialized this'; let trace = ''; function* values(x = (trace += 'p', 41)) { trace += 'b'; yield add(x); trace += 'r'; return 9; } const iterator = values(); if (trace !== 'p') throw 'body ran before resume'; const first = iterator.next(); const last = iterator.next(); if (first.value !== 42 || first.done || last.value !== 9 || !last.done || trace !== 'pbr') throw 'incorrect generator transition'; throw add(41);"),
        "native-image.cjs",
    ).unwrap();
    let typed_store_fixture = store_fixture.replace("function make(value)", "function make(value: number)")
        .replace("function scalarArithmetic(x, y)", "function scalarArithmetic(x: number, y: number): number")
        .replace("function scalarCall(x, y)", "function scalarCall(x: unknown, y: number): unknown")
        .replace("return scalarArithmetic(x, y);", "return scalarArithmetic(x as unknown as number, y);")
        .replace("set(next)", "set(next: number)")
        .replace("target.set(value);", "target.set(value as unknown as number);")
        .replace("function makeUpdates(mask)", "function makeUpdates(mask: number)")
        .replace("setMask(next)", "setMask(next: number)")
        .replace("target.setMask(2n);", "target.setMask(2n as unknown as number);")
        .replace("target.value = 1n;", "target.value = 1n as unknown as number;")
        .replace("return 1; } };", "return 1; } } as unknown as number;");
    let ts = Engine::specialize_typescript(
        &format!("{typed_store_fixture}{}", "function makeGuard(mask: number, expected: number) { function check(this: {value: number}): boolean { return (this.value & mask) !== 0 || this.value == expected; } return {check, setMask(value: number) { mask = value; }, value: 4}; } function exerciseGuard() { const guard = makeGuard(2, 4); for (let n = 0; n < 64; n++) if (!guard.check()) throw 'boolean direct'; const original = guard.check; guard.check = function() { return false; }; if (guard.check()) throw 'boolean replacement fallback'; guard.check = original; if (!guard.check(99)) throw 'boolean arguments fallback'; function tail(g) { return g.check(); } if (!tail(guard)) throw 'boolean tail fallback'; guard.setMask(4294967297); guard.value = 3; if (!guard.check()) throw 'boolean int32 fallback'; guard.setMask(1.5); guard.value = 3; if (!guard.check()) throw 'boolean fraction fallback'; guard.setMask(1n as unknown as number); let mixedThrew = false; try { guard.check(); } catch (error) { mixedThrew = error instanceof TypeError; } if (!mixedThrew) throw 'boolean bigint fallback'; } exerciseGuard(); function add(x: number): number { return x + 1; } const actual: unknown = add('x' as unknown as number); if (actual !== 'x1') throw 'incorrect coercion'; throw 42;"),
        "native-image.cts",
    ).unwrap();
    js.write_binary(&directory.join("js.qbc")).unwrap();
    ts.write_binary(&directory.join("ts.qbc")).unwrap();
    let mut js_module = lower_native_js_module(&js, "fixture_js").unwrap();
    let mut ts_module = lower_native_js_module(&ts, "fixture_ts").unwrap();
    assert_eq!(js_module.entry_table_symbol(), "fixture_js_entries");
    assert_eq!(ts_module.entry_table_symbol(), "fixture_ts_entries");
    for (program, module, namespace, image) in [
        (&js, &mut js_module, "fixture_js", 0), (&ts, &mut ts_module, "fixture_ts", 1),
    ] {
        entry_counter(module, image, "native_image_entered", |name| name == format!("{namespace}_function_0"));
        entry_counter(module, image, "native_boolean_entered", |name| name.ends_with("_boolean"));
        entry_counter(module, image, "native_scalar_entered", |name| name.contains("_scalar_"));
        let disassembly = program.disassemble();
        let store = disassembly.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next(), words.next()) {
                (Some("function"), Some(id), Some("store")) => id.parse::<u32>().ok(),
                _ => None,
            }
        }).expect("captured store function missing");
        entry_counter(module, image, "native_store_body_entered", |name| name == format!("{namespace}_function_{store}"));
    }
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
    entry_counter(&mut incompatible, 1, "native_image_entered", |name| name == "fixture_ts_function_0");
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
