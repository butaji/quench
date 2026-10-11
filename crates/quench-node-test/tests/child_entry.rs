use std::{process::Command, time::Duration};

#[test]
fn embedding_children_execute_node_arguments_instead_of_the_suite() {
    for (source, exit_code, stdout) in [
        ("console.log('child-entry')", 0, b"child-entry\n".as_slice()),
        ("process.exit(7)", 7, b"".as_slice()),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_run-parallel"));
        command
            .env(quench_node::shared_run::CHILD_RUNNER_ENV, "1")
            .args(["-e", source]);
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(exit_code));
        assert_eq!(observation.stdout, stdout);
        assert!(observation.stderr.is_empty());
    }
}

#[test]
fn node_cli_separates_runtime_options_from_guest_arguments() {
    const EVAL: &str =
        "console.log(JSON.stringify({argv:process.argv.slice(1),execArgv:process.execArgv}))";
    const PRINT: &str = "JSON.stringify({argv:process.argv.slice(1),execArgv:process.execArgv})";
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("entry.cjs"), EVAL).unwrap();
    for arguments in [
        vec!["--no-warnings", "entry.cjs", "extra"],
        vec!["--no-warnings", "-e", EVAL, "extra"],
        vec!["--no-warnings", "-p", PRINT, "extra"],
    ] {
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.current_dir(directory.path()).args(&arguments);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap()
        });
        for observation in &observations {
            assert!(!observation.timed_out);
            assert_eq!(observation.exit_code, Some(0));
            assert!(observation.stderr.is_empty());
        }
        assert_eq!(observations[0].stdout, observations[1].stdout);
    }
}

#[test]
fn package_scope_applies_inside_nested_checkouts() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("package.json"),
        r#"{"type":"module"}"#,
    )
    .unwrap();
    let nested = directory.path().join("tests/node");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("entry.js"),
        "export const value = 'package-module'; console.log(value)",
    )
    .unwrap();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command
            .current_dir(directory.path())
            .arg("tests/node/entry.js");
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0));
        assert_eq!(observation.stdout, b"package-module\n");
        assert!(observation.stderr.is_empty());
    }
}

#[test]
fn util_type_predicates_match_node_internal_brands() {
    let source = r#"
      const types = require('util').types;
      const names = ['isDate','isRegExp','isNativeError','isPromise','isMap','isSet',
        'isWeakMap','isWeakSet','isProxy','isArrayBuffer','isSharedArrayBuffer',
        'isAnyArrayBuffer','isDataView','isTypedArray','isArrayBufferView',
        'isArgumentsObject','isUint8Array','isUint8ClampedArray','isUint16Array',
        'isUint32Array','isInt8Array','isInt16Array','isInt32Array','isBigInt64Array',
        'isBigUint64Array','isFloat16Array','isFloat32Array','isFloat64Array',
        'isAsyncFunction','isGeneratorFunction','isGeneratorObject','isMapIterator',
        'isSetIterator','isModuleNamespaceObject','isBooleanObject','isNumberObject',
        'isStringObject','isSymbolObject','isBigIntObject','isBoxedPrimitive'];
      const buffer = new ArrayBuffer(8);
      const revoked = Proxy.revocable({}, {}); revoked.revoke();
      const values = [undefined,null,1,'x',{},new Date(),Object.create(Date.prototype),
        /x/,new Error(),Promise.resolve(),new Map(),new Set(),new WeakMap(),new WeakSet(),
        new Proxy(new Date(),{}),revoked.proxy,buffer,new SharedArrayBuffer(8),
        new DataView(buffer),(function(){return arguments})(),
        new Uint8Array(),new Uint8ClampedArray(),new Uint16Array(),new Uint32Array(),
        new Int8Array(),new Int16Array(),new Int32Array(),new BigInt64Array(),
        new BigUint64Array(),new Float16Array(),new Float32Array(),new Float64Array(),
        require('vm').runInNewContext('new Date()'),
        {get [Symbol.toStringTag](){throw Error('must not read tag')}},
        async function(){},function*(){},async function*(){},
        (function*(){})(),(async function*(){})(),new Map().keys(),new Map().values(),
        new Map().entries(),new Set().values(),new Set().entries(),
        Object(true),Object(2),Object('x'),Object(Symbol()),Object(2n),
        Object.create(Number.prototype),new Proxy(Object(2),{}),
        require('vm').runInNewContext('Object(2)')];
      console.log(JSON.stringify(names.map(name => [name,types[name].name,
        types[name].length,values.map(value=>types[name](value))])));
      console.log(require('util/types')===types);
    "#;
    let directory = tempfile::tempdir().unwrap();
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(observation.stderr.is_empty());
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn configured_node_library_survives_guest_cwd_changes() {
    let directory = tempfile::tempdir().unwrap();
    let library = directory.path().join("installation");
    std::fs::create_dir_all(library.join("lib/internal")).unwrap();
    std::fs::write(library.join("package.json"), r#"{"type":"module"}"#).unwrap();
    std::fs::write(library.join("lib/internal/probe.js"), "module.exports = 'library-probe';").unwrap();
    let cwd = directory.path().join("guest");
    std::fs::create_dir(&cwd).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_run-parallel"));
    command.current_dir(directory.path())
        .env(quench_node::shared_run::CHILD_RUNNER_ENV, "1")
        .env(quench_node::shared_run::NODE_LIBRARY_ROOT_ENV, "installation")
        .args(["-e", "process.chdir('guest');console.log(require('internal/probe'));console.log(require.resolve('internal/probe'))"]);
    let observation = quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap();
    assert!(!observation.timed_out);
    assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
    assert_eq!(String::from_utf8(observation.stdout).unwrap(), format!("library-probe\n{}\n", library.join("lib/internal/probe.js").display()));
    assert!(observation.stderr.is_empty());
}
