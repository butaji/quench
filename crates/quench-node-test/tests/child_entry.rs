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
        assert_eq!(
            String::from_utf8_lossy(&observations[0].stdout),
            String::from_utf8_lossy(&observations[1].stdout)
        );
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
fn builtin_esm_exports_keep_node_snapshots_of_lazy_members() {
    // `node:fs` builds its streams and library policy on first use; its ESM exports must
    // still be the values the members had when the facade was made (Node's `syncExports`).
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("reexport.mjs"), "export * from 'node:fs';").unwrap();
    for (name, source, expected) in [
        (
            "read",
            "import * as fs from 'node:fs'; import { ReadStream as R } from './reexport.mjs';
             import { createRequire } from 'node:module';
             const cjs = createRequire(import.meta.url)('fs');
             const d = Object.getOwnPropertyDescriptor(fs, 'WriteStream');
             let n = 0; for (let i = 0; i < 2000; i++) if (typeof fs.createWriteStream === 'function') n++;
             console.log(typeof fs.ReadStream, fs.ReadStream === cjs.ReadStream, fs.ReadStream.name,
               R === fs.ReadStream, typeof d.value, d.writable, d.enumerable, d.configurable,
               d.value.name, Object.keys(fs).includes('createReadStream'), n);",
            "function true ReadStream true function true true false WriteStream true 2000\n",
        ),
        (
            "write",
            "import * as fs from 'node:fs'; import { createRequire } from 'node:module';
             const cjs = createRequire(import.meta.url)('fs');
             cjs.ReadStream = 1; delete cjs.WriteStream; cjs.glob = 'g';
             console.log(typeof fs.ReadStream, cjs.ReadStream, typeof fs.WriteStream,
               'WriteStream' in cjs, typeof fs.glob, cjs.glob);",
            "function 1 function false function g\n",
        ),
        (
            "named",
            "import { createReadStream, glob } from 'node:fs'; import * as fs from 'node:fs';
             console.log(typeof createReadStream, typeof glob, createReadStream === fs.createReadStream);",
            "function function true\n",
        ),
        (
            "materialized",
            "import { createRequire } from 'node:module';
             const cjs = createRequire(import.meta.url)('fs');
             const original = cjs.WriteStream; const fs = await import('node:fs');
             cjs.WriteStream = 2; console.log(fs.WriteStream === original, cjs.WriteStream);",
            "true 2\n",
        ),
    ] {
        let entry = directory.path().join(format!("{name}.mjs"));
        std::fs::write(&entry, source).unwrap();
        for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
            let mut command = Command::new(executable);
            command.arg(&entry);
            if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
            let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap();
            assert_eq!(result.exit_code, Some(0), "{name} {executable}: {}", String::from_utf8_lossy(&result.stderr));
            assert_eq!(String::from_utf8_lossy(&result.stdout), expected, "{name} {executable}");
        }
    }
}

#[test]
fn package_scope_reads_nearest_type_and_stops_at_node_modules() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("package.json"), r#"{"type":"module"}"#).unwrap();
    for (name, config, expected) in [
        ("inherited", None, "undefined\n"),
        ("empty", Some("{}"), "function\n"),
        ("common", Some(r#"{"type":"commonjs"}"#), "function\n"),
        ("unknown", Some(r#"{"type":"other"}"#), "function\n"),
        ("duplicate", Some(r#"{"type":"commonjs","type":"module"}"#), "undefined\n"),
        ("bom", Some("\u{feff}{\"type\":\"module\"}"), "undefined\n"),
        ("node_modules/plain", None, "function\n"),
        ("node_modules/explicit", Some(r#"{"type":"module"}"#), "undefined\n"),
    ] {
        let scope = directory.path().join(name);
        std::fs::create_dir_all(scope.join("nested")).unwrap();
        if let Some(config) = config { std::fs::write(scope.join("package.json"), config).unwrap(); }
        let entry = scope.join("nested/entry.js");
        std::fs::write(&entry, "console.log(typeof require)").unwrap();
        for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
            let mut command = Command::new(executable);
            command.arg(&entry);
            if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
            let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap();
            assert_eq!(result.exit_code, Some(0), "{name}: {}", String::from_utf8_lossy(&result.stderr));
            assert_eq!(result.stdout, expected.as_bytes(), "{name}");
            assert!(result.stderr.is_empty(), "{name}");
        }
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn configured_node_library_survives_guest_cwd_changes() {
    let directory = tempfile::tempdir().unwrap();
    let library = directory.path().join("installation");
    std::fs::create_dir_all(library.join("lib/internal")).unwrap();
    std::fs::write(library.join("package.json"), r#"{"type":"module"}"#).unwrap();
    std::fs::write(
        library.join("lib/internal/probe.js"),
        "module.exports = 'library-probe';",
    )
    .unwrap();
    let cwd = directory.path().join("guest");
    std::fs::create_dir(&cwd).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_run-parallel"));
    command.current_dir(directory.path())
        .env(quench_node::shared_run::CHILD_RUNNER_ENV, "1")
        .env(quench_node::shared_run::NODE_LIBRARY_ROOT_ENV, "installation")
        .args(["-e", "process.chdir('guest');console.log(require('internal/probe'));console.log(require.resolve('internal/probe'))"]);
    let observation =
        quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap();
    assert!(!observation.timed_out);
    assert_eq!(
        observation.exit_code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&observation.stderr)
    );
    assert_eq!(
        String::from_utf8(observation.stdout).unwrap(),
        format!(
            "library-probe\n{}\n",
            library.join("lib/internal/probe.js").display()
        )
    );
    assert!(observation.stderr.is_empty());
}

#[test]
fn deep_comparison_modes_match_node_values_keys_and_prototypes() {
    let source = r#"
      const assert = require('assert'), util = require('util');
      const symbol = Symbol();
      class A { constructor(){this.x=1} } class B { constructor(){this.x=1} }
      const a={x:1},b={x:1};a.self=a;b.self=b;
      const bytes = value => new Uint8Array(value).buffer;
      const cases = [[new Date(1),new Date(2)],[new Date(NaN),new Date(NaN)],
        [Object(1),Object(2)],[Object(0),Object(-0)],[Object(NaN),Object(NaN)],
        [/a/g,/b/g],[/a/g,/a/g],[new Error('a'),new Error('b')],
        [new Error('a'),new Error('a',{cause:undefined})],
        [new Map([[{x:1},2]]),new Map([[{x:1},2]])],
        [new Map([[1,2]]),new Map([[1,3]])],
        [new Set([{x:1},2]),new Set([2,{x:1}])],
        [new Set([1]),new Set([2])],[new WeakMap(),new WeakMap()],
        [Promise.resolve(1),Promise.resolve(1)],
        [new Uint8Array([1,2]),new Uint8Array([1,3])],
        [new Uint8Array([1,2]),new Uint16Array([513])],
        [new Float32Array([0]),new Float32Array([-0])],
        [new Float64Array([NaN]),new Float64Array([NaN])],
        [bytes([1,2]),bytes([1,3])],
        [new DataView(bytes([1,2])),new DataView(bytes([1,3]))],
        [new URL('http://foo'),new URL('http://bar')],[new URL('http://foo'),new URL('http://foo')],
        [1,'1'],[NaN,NaN],[0,-0],[1n,1],[Symbol(),Symbol()],
        [()=>{},()=>{}],[new A(),new B()],[a,b],[new Array(1),new Array(2)],
        [[,],[undefined]],[[,],[,]], [{[symbol]:1},{[symbol]:2}],
        [{[symbol]:1},{[Symbol()]:1}], [{x:1},{x:'1'}],
        [Object.defineProperty({x:1},symbol,{value:2}),{x:1}]];
      function outcome(fn){try{fn();return true}catch(error){return error.code}}
      console.log(JSON.stringify(cases.map(([a,b]) => [
        outcome(()=>assert.deepEqual(a,b)),outcome(()=>assert.notDeepEqual(a,b)),
        outcome(()=>assert.deepStrictEqual(a,b)),outcome(()=>assert.notDeepStrictEqual(a,b)),
        util.isDeepStrictEqual(a,b),util.isDeepStrictEqual(a,b,true)])));
      console.log(JSON.stringify([
        [new Error('a'),new Error('a',{cause:undefined})],
        [new Error('a',{cause:1}),new Error()],
        [{x:1,y:2},{x:1}], [{x:1},{x:1,y:2}],
        [[1,2,3],[1,3]], [[,1,,2],[,1,1]], [[,1,,2],[,1,2]],
        [[undefined],[,]], [[,],[undefined]], [[1,2],[2,1]],
        [new Uint8Array([1,2,3]),new Uint8Array([1,3])],
        [new Set([{x:1,y:2},3]),new Set([{x:1}])],
        [new Map([[{x:1,y:2},{a:1,b:2}]]),new Map([[{x:1},{a:1}]])]
      ].map(([a,b])=>outcome(()=>assert.partialDeepStrictEqual(a,b)))));
      console.log(JSON.stringify(['equal','notEqual','deepEqual','notDeepEqual'].map(name=>outcome(()=>assert[name]()))));
      console.log(outcome(()=>assert.equal(NaN,NaN)),util.isDeepStrictEqual());
      Object.keys=Object.getOwnPropertySymbols=Object.getPrototypeOf=Array.isArray=()=>{throw Error('patched')};
      console.log(outcome(()=>assert.deepStrictEqual({x:1},{x:1})),
        outcome(()=>assert.deepStrictEqual([1],[1])));
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn util_text_operations_match_node_without_utf16_loss() {
    let source = r#"
      const util = require('util');
      const texts = ['', 'plain', '\x1b', '\x1b[', '\x1b[31mred\x1b[0m',
        '\x1b(Btext', '\x9b2Jtext', '\x1b]8;;https://a/b?x=1&y=2\x07link\x1b]8;;\x07',
        '\x1b]8;;https://a\x1b\\link\x1b]8;;\x1b\\', '\ud800\x1b[31m\udfff',
        '\x1b[99999mtext', '\x1b[31☃text', '\x1b]unterminated', '😀\x1b[0m😀',
        '\x1b]8;;https://example.com/(foo\x07label\x1b]8;;\x07',
        '\x1b]8;;https://example.com/foo)bar\x9clabel\x1b]8;;\x9c',
        '\x1b[38:2:255:0:0mHello\x1b[0m', '\x1b[4:3mUnderline\x1b[4:0m'];
      console.log(JSON.stringify(texts.map(text=>util.stripVTControlCharacters(text))));
      const envs = ['', 'Z=1\nA=2\nZ=3', 'A=1#comment\nB=x #rest',
        'A=`x\\n`\nB="x\\n\\r"\nC=\'x\\n\'', 'A="one\ntwo"\nB=three',
        'bad key=x\n1key=y\n=value\nexport   Z=z', 'A=\nB=2', 'A="unterminated\nB=2',
        'A=x\r\nB=y\r\n', '\t #comment\n X = y ', 'A="a"ignored\nB=b'];
      console.log(JSON.stringify(envs.map(text=> {
        const result=util.parseEnv(text);
        return [Object.getPrototypeOf(result)===null,Object.entries(result),
          Object.values(Object.getOwnPropertyDescriptors(result)).map(d=>[d.writable,d.enumerable,d.configurable])];
      })));
      const fs = require('fs');
      for (const key of ['QUENCH_ORACLE_A','QUENCH_ORACLE_B','QUENCH_ORACLE_C']) delete process.env[key];
      fs.writeFileSync('oracle.env', 'QUENCH_ORACLE_A=first\nQUENCH_ORACLE_A=last#comment\nQUENCH_ORACLE_B="one\\ntwo"\nQUENCH_ORACLE_C=`keep\\n`');
      process.loadEnvFile('oracle.env');
      console.log(JSON.stringify(['QUENCH_ORACLE_A','QUENCH_ORACLE_B','QUENCH_ORACLE_C'].map(key=>process.env[key])));
      console.log(JSON.stringify(['stripVTControlCharacters','parseEnv'].map(name=>[
        util[name].name,util[name].length,
        [undefined,null,{},[],42,true,1n,Symbol('x')].map(value=>{
          try{util[name](value);return 'pass'}catch(e){return [e.name,e.code,e.message]}
        })])));
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn callbackify_matches_node_descriptors_receivers_and_tick_order() {
    let source = r#"
      const { callbackify } = require('util');
      const marker = Symbol('marker'), log = [];
      function original(a) { log.push(['original', this.tag, a]); return Promise.resolve(a); }
      Object.defineProperty(original, 'hidden', {value:marker, enumerable:false});
      original[marker] = 17;
      const cb = callbackify(original);
      console.log(JSON.stringify([cb.name,cb.length,cb.prototype===original.prototype,
        Object.getPrototypeOf(cb)===Function.prototype,cb.hidden===marker,cb[marker],
        Object.getOwnPropertyDescriptor(cb,'hidden'),
        Object.getOwnPropertyDescriptor(cb,'name'),Object.getOwnPropertyDescriptor(cb,'length')]));
      const receiver = {tag:'receiver'};
      console.log(cb.call(receiver, 7, function(err,value){log.push(['resolved',this.tag,err,value,arguments.length])}));
      callbackify(function(){'use strict';return Promise.resolve(this===undefined)})(function(err,value){'use strict';log.push(['plain',this===undefined,err,value])});
      const thenable = callbackify(function(){return {then(resolve){resolve('thenable')}}});
      thenable.call(receiver, function(err,value){log.push(['thenable',this.tag,err,value])});
      for (const value of [null,undefined,false,0,-0,'',NaN,0n]) {
        callbackify(()=>Promise.reject(value)).call(receiver, function(err){
          log.push(['rejected',this.tag,err.code,err.message,Object.is(err.reason,value),arguments.length,err.stack.includes('at process.processTicksAndRejections')]);
        });
      }
      function getterName(){return Promise.resolve(1)}
      Object.defineProperty(getterName,'name',{get(){return 'getter-name'},enumerable:true});
      const copied = callbackify(getterName);
      console.log(copied.name,Object.getOwnPropertyDescriptor(copied,'name').get===Object.getOwnPropertyDescriptor(getterName,'name').get);
      for (const value of [undefined,null,'foo',false,0,{},Symbol(),[]]) {
        try{callbackify(value)}catch(e){console.log(e.name,e.code,e.message)}
      }
      try{cb(1)}catch(e){console.log(e.name,e.code,e.message)}
      process.nextTick(()=>log.push(['tick']));
      Promise.resolve().then(()=>log.push(['microtask']));
      process.on('exit',()=>console.log(JSON.stringify(log)));
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn error_constructors_capture_stack_at_the_node_observable_phase() {
    let source = r#"
      function capture(Ctor) {
        const error = Ctor===AggregateError ? new Ctor([],'message') : new Ctor('message');
        const descriptor = Object.getOwnPropertyDescriptor(error,'stack');
        console.log(JSON.stringify([error.name,error.stack.includes('at capture'),
          descriptor.enumerable,descriptor.configurable,typeof descriptor.get,typeof descriptor.set]));
      }
      for(const Ctor of [Error,TypeError,RangeError,AggregateError]) capture(Ctor);
      Error.stackTraceLimit=5;
      const first=new Error({toString(){Error.stackTraceLimit=0;return 'x'}});
      console.log(first.stack.split('\n').length);
      Error.stackTraceLimit=5;
      const second=new Error('x',{get cause(){Error.stackTraceLimit=0;return 1}});
      console.log(second.stack.split('\n').length);
      Error.stackTraceLimit=5;
      const third=new AggregateError({[Symbol.iterator](){Error.stackTraceLimit=0;return [][Symbol.iterator]()}},'x');
      console.log(third.stack.split('\n').length>1);
      console.log(Object.getOwnPropertyDescriptor(Error.prototype,'stack')===undefined);
      let count=0;
      Error.prepareStackTrace=(error,sites)=>{count++;return {count,sites}};
      const cached=new Error;
      console.log(cached.stack===cached.stack,count);
      const accessor=Object.getOwnPropertyDescriptor(cached,'stack');
      for(const value of [123,null,undefined,{x:1}]) { cached.stack=value; console.log(cached.stack===value,typeof Object.getOwnPropertyDescriptor(cached,'stack').get); }
      console.log(accessor.get.call({}),accessor.get.call(null),accessor.set.call({},42));
      Error.stackTraceLimit=5;
      Error.prepareStackTrace=(error,sites)=>sites.slice(0,2).map(site=>[site.getFunctionName(),site.getThis()===undefined]);
      function outer(){return inner.call({x:1})}
      function inner(){'use strict';return new Error().stack}
      console.log(JSON.stringify(outer.call({x:2})));
      Error.prepareStackTrace=(error,sites)=>sites.map(site=>site.getFunctionName());
      function omitted(){const object={};Error.captureStackTrace(object,omitted);return object.stack}
      function caller(){return omitted()}
      console.log(caller().includes('caller'),caller().includes('omitted'));
      const missing={};Error.captureStackTrace(missing,function absent(){});console.log(missing.stack.length);
      const sharedA=Object.getOwnPropertyDescriptor(new Error,'stack');
      const sharedB=Object.getOwnPropertyDescriptor(new Error,'stack');
      console.log(sharedA.get===sharedB.get,sharedA.set===sharedB.set,sharedA.get.name,sharedA.set.name);
      delete Error.prepareStackTrace;
      for(const limit of [undefined,null,'5',NaN,-1,0,Infinity]) {
        Error.stackTraceLimit=limit;
        const error=new Error('limit');
        console.log(typeof error.stack,error.stack===undefined || error.stack.split('\n').length===1);
        error.stack=42;console.log(error.stack);
      }
      let reads=0;
      Object.defineProperty(Error,'stackTraceLimit',{configurable:true,get(){reads++;return 5}});
      console.log(new Error().stack,reads);
      Object.defineProperty(Error,'stackTraceLimit',{configurable:true,writable:true,value:5});
      const OriginalError=Error;
      OriginalError.prepareStackTrace=()=>123;
      globalThis.Error=function ReplacementError(){};
      console.log(new OriginalError().stack);
      globalThis.Error=OriginalError;


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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn exec_file_matches_node_output_limits_errors_and_completion() {
    let source = r#"const {execFile}=require('child_process');
const {Buffer}=require('buffer');
const results=[];
function record(name,error,stdout,stderr){
  results.push([name,error ? [error.name,error.code,error.signal??null,error.killed??null,typeof error.cmd==='string',Object.hasOwn(error,'stdout')] : null,
    typeof stdout,Buffer.isBuffer(stdout)?stdout.toString('hex'):stdout,
    typeof stderr,Buffer.isBuffer(stderr)?stderr.toString('hex'):stderr]);
  if(results.length===7)console.log(JSON.stringify(results.sort((a,b)=>a[0].localeCompare(b[0]))));
}
execFile(process.execPath,['-e','process.stdout.write("out");process.stderr.write("err")'],(err,out,e)=>record('text',err,out,e));
execFile(process.execPath,['-e','process.stdout.write("é")'],{encoding:null},(err,out,e)=>record('bytes',err,out,e));
execFile(process.execPath,['-e','process.stderr.write("failure");process.exitCode=3'],(err,out,e)=>record('failure',err,out,e));
execFile('/quench-nonexistent-exec-file',[],(err,out,e)=>record('missing',err,out,e));
try{execFile(process.execPath,[],{},123)}catch(error){console.log(error.code,error.name)}

execFile(process.execPath,['-e','process.stdout.write("abcdefghij")'],{maxBuffer:4},(err,out,e)=>record('limit',err,out,e));
execFile(process.execPath,['-e','setInterval(()=>{},1000)'],{timeout:20},(err,out,e)=>record('timeout',err,out,e));
const changed=execFile(process.execPath,['-e','process.stdout.write("é")'],{encoding:null},(err,out,e)=>record('changed',err,out,e));
changed.stdout.setEncoding('utf8');
for(const signal of [null,'hello',{},()=>{}]) {
  try{execFile(process.execPath,[],{signal},()=>{})}catch(error){console.log(error.name,error.code,error.message)}
}
const marker={};
try{execFile(process.execPath,[],{signal:new Proxy({},{has(){throw marker}})},()=>{})}catch(error){console.log(error===marker)}
const descriptor=Object.getOwnPropertyDescriptor(require('stream').Readable.prototype,'readableEncoding');
console.log(descriptor.get.name,descriptor.enumerable,descriptor.configurable,descriptor.get.call({}));

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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn exec_normalization_and_collection_match_node() {
    let source = r#"const cp=require('child_process');
const inspect=require('util').inspect;
for(const text of [...Array.from({length:256},(_,i)=>String.fromCharCode(i)),"a'b",'a\'"b','a\'"`b','a\'"${b}', '\ud800x\udfff', '\ud83d\ude00']) console.log(JSON.stringify(inspect(text)));
const original=cp.execFile;
const callback=()=>{};
const marker={};
cp.execFile=function(command,options,cb){
  console.log(JSON.stringify([this===cp,command,Object.getPrototypeOf(options)===null,Object.keys(options),options.shell,cb===callback]));
  return marker;
};
for(const options of [undefined,null,false,1,'x',{shell:false},{shell:'/bin/sh'},callback]) {
  console.log(cp.exec('printf hello',options,callback)===marker);
}
const options={shell:false}; cp.exec('printf hello',options,callback); console.log(options.shell===false);
for(const command of [undefined,null,1,{},'a\0b']) {
  try{cp.exec(command)}catch(error){console.log(JSON.stringify([error.name,error.code,error.message]))}
}
cp.execFile=original;
const results=[];
function record(name,error,out,err){
  results.push([name,error?[error.name,error.code,error.signal??null,error.killed??null]:null,typeof out,Buffer.isBuffer(out)?out.toString('hex'):out,typeof err,Buffer.isBuffer(err)?err.toString('hex'):err]);
  if(results.length===5)console.log(JSON.stringify(results.sort((a,b)=>a[0].localeCompare(b[0]))));
}
cp.exec('printf out; printf err >&2',(e,o,s)=>record('text',e,o,s));
cp.exec('printf out',{encoding:null},(e,o,s)=>record('bytes',e,o,s));
cp.exec('printf failure >&2; exit 3',(e,o,s)=>record('failure',e,o,s));
cp.exec('printf abcdef',{maxBuffer:3},(e,o,s)=>record('limit',e,o,s));
cp.exec('sleep 1',{timeout:20},(e,o,s)=>record('timeout',e,o,s));
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn promisified_child_execution_matches_node_capabilities_and_validation() {
    let source = r#"const cp=require('child_process');
const {promisify}=require('util');
for(const original of [cp.exec,cp.execFile]) {
  const descriptor=Object.getOwnPropertyDescriptor(original,promisify.custom);
  const fn=promisify(original);
  console.log(JSON.stringify([fn===descriptor.value,fn===promisify(original),fn.name,fn.length,Object.hasOwn(fn,'prototype'),descriptor.writable,descriptor.enumerable,descriptor.configurable]));
  for(const signal of [{},()=>{}]) {
    try{fn('printf hello',{signal})}catch(error){console.log(error.name,error.code)}
  }
}
const exec=promisify(cp.exec), execFile=promisify(cp.execFile);
Promise.withResolvers=()=>{throw new Error('replaced public capability factory')};
const results=[];
function record(name,promise) {
  console.log(name,typeof promise.child.pid);
  promise.then(({stdout,stderr})=>{
    results.push([name,null,stdout,stderr]);finish();
  },error=>{
    results.push([name,[error.name,error.code,Object.hasOwn(error,'stdout'),Object.hasOwn(error,'stderr')],error.stdout,error.stderr]);finish();
  });
}
function finish(){if(results.length===3)console.log(JSON.stringify(results.sort((a,b)=>a[0].localeCompare(b[0]))));}
record('exec',exec('printf out; printf err >&2'));
record('file',execFile(process.execPath,['-e','process.stdout.write("file")']));
record('failure',exec('printf failure >&2; exit 3'));
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
#[cfg(unix)]
fn destroyed_child_streams_release_shell_descendant_close_waiting() {
    let source = r#"const {exec}=require('child_process');
const {promisify}=require('util');
let finished=0;
function done(){if(++finished===2)console.log('closed');}
exec('sleep 3',{timeout:30},(error,stdout,stderr)=>{
  console.log(error.code,error.signal,error.killed,JSON.stringify(stdout),JSON.stringify(stderr));done();
});
const controller=new AbortController();
const promise=promisify(exec)('sleep 3',{signal:controller.signal});
promise.catch(error=>{console.log(error.name,error.code,error.cause,JSON.stringify(error.stdout),JSON.stringify(error.stderr));done();});
controller.abort('cause');
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(2)).unwrap()
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
#[cfg(unix)]
fn exec_reports_kill_override_errors_through_its_callback() {
    let source = r#"const cp=require('child_process');
const internal=require('internal/child_process');
console.log(internal.ChildProcess===cp.ChildProcess);
const marker=new Error('kill override');
const kill=cp.ChildProcess.prototype.kill;
cp.ChildProcess.prototype.kill=function(...args){kill.apply(this,args);throw marker;};
const command=`"${process.execPath}" -e 'console.log("output");setInterval(()=>{},1000)'`;
const child=cp.exec(command,{maxBuffer:0,killSignal:'SIGKILL'},(error,out,err)=>{
  console.log(error===marker,error.message,out==='',err==='',child.killed,child instanceof cp.ChildProcess,error.cmd===command);
});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-internals", "-e", source]);
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
#[cfg(unix)]
fn synchronous_child_output_limits_match_node_values_and_errors() {
    let source = r#"const {spawnSync,execFileSync}=require('child_process');
for(const maxBuffer of [0,1,Infinity]) {
  const args=['-e','process.stdout.write("hello")'];
  const result=spawnSync(process.execPath,args,{maxBuffer});
  const error=result.error;
  console.log(JSON.stringify([maxBuffer,result.stdout.toString(),error?[error.name,error.code,error.errno,error.syscall===`spawnSync ${process.execPath}`,error.path===process.execPath,error.spawnargs]:null]));
  try{console.log(execFileSync(process.execPath,args,{maxBuffer}).toString())}catch(error){console.log(error.name,error.code,error.errno,error.stdout.toString())}
}
const combined=spawnSync(process.execPath,['-e','process.stdout.write("a".repeat(16));process.stderr.write("b".repeat(16))'],{maxBuffer:24});
console.log(combined.error.code,combined.stdout.length+combined.stderr.length>24);
for(const maxBuffer of [-1,NaN,'2',null])try{spawnSync(process.execPath,[],{maxBuffer})}catch(error){console.log(error.name,error.code,error.message)}
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn assertion_exception_objects_share_strict_property_comparison() {
    let source = r#"const assert=require('assert');
function check(actual,expected){
  try{assert.throws(()=>{throw actual},expected);console.log('match')}
  catch(error){console.log(error.name,error.code,error.operator,error.actual===actual,error.expected===expected)}
}
check({message:'hello'},{message:/ell/});
check({message:'actual'},{message:'expected'});
check({nested:{a:1,b:2}},{nested:{a:1}});
check({value:undefined},{value:undefined});
check({},{value:undefined});
check(undefined,{value:undefined});
check({value:3},{value:/3/});
check({value:new Number(3)},{value:3});
const actual={};actual.value={next:actual};const expected={};expected.value={next:expected};check(actual,expected);
const events=[];
const a=new Proxy({value:'hello'},{get(target,key){events.push('get:'+String(key));return target[key]},has(target,key){events.push('has:'+String(key));return key in target}});
const e={get value(){events.push('expected');return 'hello'}};
check(a,e);console.log(JSON.stringify(events));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn util_styles_match_node_ansi_aliases_cache_and_iterator_protocol() {
    let source = r#"const util=require('util');const colors=util.inspect.colors;
function print(format,text,options){try{console.log(JSON.stringify(util.styleText(format,text,options)))}catch(error){console.log(error.name,error.code,error.message)}}
for(const format of Object.getOwnPropertyNames(colors))print(format,'text',{validateStream:false});
for(const format of ['#fc0','#AAbbCc','#xyz','invalid',null,undefined])print(format,'text',{validateStream:false});
print(['bold','red'],'A'+util.styleText('blue','B',{validateStream:false})+'C',{validateStream:false});
print('dim',util.styleText('bold','bold',{validateStream:false})+'dim',{validateStream:false});
print('red','\ud800',{validateStream:false});
print('red','text',{validateStream:0});print(['red'],'text',{validateStream:0});
print('red','text',{stream:{}});
const alias=Object.getOwnPropertyDescriptor(colors,'grey');console.log(alias.enumerable,alias.configurable,alias.get.name,alias.set.name,colors.grey===colors.gray);
colors.red=[99,39];print('red','cached',{validateStream:false});print(['red'],'live',{validateStream:false});
const events=[];const formats=['unused'];formats[Symbol.iterator]=()=>({get next(){events.push('next getter');let n=0;return()=>{events.push('next');return n++?{done:true}:{done:false,value:'blue'}}}});
print(formats,'iterator',{validateStream:false});console.log(JSON.stringify(events));
const failing=['unused'];failing[Symbol.iterator]=()=>({next(){return {done:false,value:'invalid'}},return(){console.log('closed iterator');return {}}});print(failing,'text',{validateStream:false});
const stream={write(){},on(){},isTTY:true,getColorDepth(){return {valueOf(){return 4}}}};print('red','stream',{stream});
stream.getColorDepth=()=>3n;print('red','bigint depth',{stream});
console.log(typeof process.stdout.on,process.stdout instanceof require('events').EventEmitter);
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn oversized_read_file_returns_node_range_error_without_allocating() {
    let directory = tempfile::tempdir().unwrap();
    let file = std::fs::File::create(directory.path().join("large")).unwrap();
    file.set_len(i32::MAX as u64 + 1).unwrap();
    std::fs::write(directory.path().join("small"), b"small").unwrap();
    let source = r#"const fs=require('fs');
function record(error){console.log(error.name,error.code,error.message)}
try{fs.readFileSync('large')}catch(error){record(error)}
fs.readFile('large',error=>record(error));
fs.promises.readFile('large').catch(error=>record(error));
console.log(fs.readFileSync('small','utf8'));
"#;
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
    let mut lines = observations.map(|observation| {
        String::from_utf8(observation.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    for lines in &mut lines {
        lines.sort();
    }
    assert_eq!(lines[0], lines[1]);
}

#[test]
fn system_error_names_and_validation_match_node() {
    let source = r#"const {getSystemErrorName}=require('util');
const numbers=[...Array.from({length:140},(_,i)=>-i-1),...Array.from({length:16},(_,i)=>-3000-i),-4094,-4095,-99999];
const result=numbers.map(value=>[value,getSystemErrorName(value)]);
for(const value of [0,1,1.5,-1.5,NaN,-Infinity,undefined,'x',-0,-9007199254740992,-1e100]){
 try{result.push(['invalid',getSystemErrorName(value)])}catch(error){result.push(['invalid',error.name,error.code,error.message])}
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn shell_argument_deprecation_is_shared_and_emitted_once() {
    let source = r#"
      const {spawnSync,execFile}=require('child_process');
      const seen=[];
      process.emitWarning=(...args)=>seen.push(args);
      spawnSync(process.execPath,[],{shell:true});
      console.log(seen.length);
      spawnSync(process.execPath,['--version'],{shell:true});
      console.log(seen.length);
      execFile(process.execPath,['--version'],{shell:true},(error)=>{
        console.log(error===null,JSON.stringify(seen));
      });
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn handled_exceptions_do_not_read_stack_or_coerce_thrown_values() {
    let source = r#"
      let reads=0,coercions=0;
      const marker={get message(){reads++;throw 1},toString(){coercions++;throw 2}};
      try{throw marker}catch(error){console.log(error===marker,reads,coercions)}
      const error=new Error('handled');
      Object.defineProperty(error,'stack',{get(){reads++;throw 3}});
      error.toString=()=>{coercions++;throw 4};
      process.on('uncaughtException',value=>console.log(value===error,reads,coercions));
      process.nextTick(()=>{throw error});
    "#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn fatal_source_excerpts_use_compiler_snapshots_and_commonjs_origins() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rewritten.cjs");
    let source = "require('fs').writeFileSync(__filename,'rewritten after compilation');\nthrow new Error('source snapshot');\n";
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        std::fs::write(&path, source).unwrap();
        let mut command = Command::new(executable);
        command.arg(&path);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(1));
        assert!(observation.stdout.is_empty());
    }
    let diagnostics =
        observations.map(|observation| String::from_utf8(observation.stderr).unwrap());
    assert_eq!(
        diagnostics[0].lines().take(3).collect::<Vec<_>>(),
        diagnostics[1].lines().take(3).collect::<Vec<_>>()
    );
    for diagnostic in &diagnostics {
        assert!(diagnostic.contains("Error: source snapshot"));
    }
    let location = format!("{}:2:", path.display());
    let positions = diagnostics.each_ref().map(|diagnostic| {
        diagnostic.lines().find_map(|line| {
            line.find(&location)
                .map(|start| line[start..].trim_end_matches(')').to_owned())
        })
    });
    assert!(positions[0].is_some());
    assert_eq!(positions[0], positions[1]);
}

#[test]
fn read_file_abort_and_callback_exceptions_match_node() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("small"), b"small").unwrap();
    let source = r#"const fs=require('fs'); const result=[];
for(const signal of [null,'bad',{},()=>{}]) {
  try {fs.readFile('small',{signal},()=>result.push('unexpected'));}
  catch(error){result.push([error.name,error.code,error.message]);}
}
const marker={reason:true}; const controller=new AbortController(); controller.abort(marker);
fs.readFile('small',{signal:controller.signal},function(error){'use strict';
 result.push([error.name,error.code,error.cause===marker,Object.getOwnPropertyDescriptor(error,'cause').enumerable,this===undefined]);
});
const later=new AbortController();
fs.readFile('small',{signal:later.signal},error=>result.push(['later',error.name]));
process.nextTick(()=>later.abort());
let calls=0; const thrown=new Error('callback');
process.on('uncaughtException',error=>result.push(['uncaught',error===thrown,calls]));
fs.readFile('small',function(error,data){calls++;throw thrown;});
process.on('exit',()=>console.log(JSON.stringify(result.map(value=>JSON.stringify(value)).sort())));
"#;
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
fn exposed_gc_preserves_weak_targets_until_job_checkpoint() {
    let source = r#"const descriptor=Object.getOwnPropertyDescriptor(globalThis,'gc');
console.log('gc',gc.name,gc.length,descriptor.writable,descriptor.enumerable,descriptor.configurable,'prototype' in gc);
const first=new WeakRef({});
gc(); console.log('sync',first.deref()!==undefined);
process.nextTick(()=>{gc();console.log('tick',first.deref()!==undefined)});
Promise.resolve().then(()=>{gc();console.log('promise',first.deref()!==undefined)});
setTimeout(()=>{gc();console.log('timer',first.deref()===undefined);
 const target=new WeakRef({});gc();console.log('new',target.deref()!==undefined);
 setTimeout(()=>{const value=target.deref();gc();console.log('deref',value===undefined||target.deref()===value)},0);
},10);
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", source]);
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
fn tcp_write_completion_retains_buffers_and_applies_backpressure() {
    let source = r#"const net=require('net'); const result=[];
let sent=0,writes=0,callbacks=0,synchronous=true,pressure=false;
const server=net.createServer(socket=>{let received=0;
 socket.on('data',chunk=>received+=chunk.length);
 socket.on('end',()=>result.push(['received',received===sent]));
 socket.resume();server.close();
}).listen(0,()=>{
 const socket=net.connect(server.address().port,()=>{
  let queued=true;
  for(let index=0;index<64&&queued;index++) {
   writes++;sent+=262144;
   queued=socket.write(Buffer.alloc(262144,7),()=>{
    callbacks++;if(synchronous)result.push(['unexpected-sync']);
    if(callbacks===writes){result.push(['callbacks',true]);socket.end();}
   });
  }
  pressure=!queued; result.push(['pressure',pressure]);
  synchronous=false;gc();
 });
});
process.on('exit',()=>console.log(JSON.stringify(result.map(value=>JSON.stringify(value)).sort())));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn canceled_preconnect_write_matches_node_callback_error() {
    let source = r#"const net=require('net');const result=[];
const server=net.createServer(socket=>socket.destroy()).listen(0,()=>{
 const socket=net.connect(server.address().port);const marker=new Error('destroy marker');
 let calls=0;
 socket.on('error',error=>result.push(['event',error===marker]));
 socket.write('abc',error=>{calls++;result.push(['callback',calls,error.name,error.code,error.message,Object.keys(error)]);});
 socket.destroy(marker);server.close();
});process.on('exit',()=>console.log(JSON.stringify(result.map(value=>JSON.stringify(value)).sort())));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
fn process_kill_matches_node_coercion_errors_and_mutable_native_hook() {
    let source = r#"const result=[];const original=process._kill;
function attempt(pid,signal){const trace=[];process._kill=function(actual,number){trace.push(['native',actual===pid,number,this===process]);return 0};
 try{result.push(['success',typeof pid,typeof signal,process.kill(pid,signal),trace]);}
 catch(error){result.push(['error',error.name,error.code,error.message,trace]);}}
for(const pid of [null,undefined,'SIGTERM',NaN,Infinity,-Infinity,1.5,2147483648,0,'0',false,true,1n,Symbol('pid')])attempt(pid,0);
for(const signal of [undefined,null,false,NaN,0,1,65,-1,1.5,'SIGINT','sigint','foo',{},Symbol('sig')])attempt(7,signal);
const coercions=[];const pid={ [Symbol.toPrimitive](hint){coercions.push(['pid',hint]);return 7} };
const signal={ [Symbol.toPrimitive](hint){coercions.push(['signal',hint]);return hint==='number'?0:'SIGINT'} };
attempt(pid,signal);result.push(['coercions',coercions]);
const lookupTrace=[];let lookup=0;const dynamicSignal={ [Symbol.toPrimitive](hint){lookupTrace.push(hint);return hint==='number'?0:(++lookup===1?'SIGINT':'missing')} };
Object.defineProperty(process,'_kill',{configurable:true,get(){lookupTrace.push('get');return function(actual,number){lookupTrace.push(['call',actual,typeof number,this===process]);return 0}}});
process.kill(7,dynamicSignal);result.push(['lookup-order',lookupTrace]);
Object.defineProperty(process,'_kill',{value:original,writable:true,configurable:true});
process._kill=function(){return 0};const constructed=new process.kill(7,0);result.push(['shape',process.kill.name,process.kill.length,constructed instanceof process.kill]);
const marker={};process._kill=()=>{throw marker};try{process.kill(7,0)}catch(error){result.push(['throw',error===marker])}
process._kill=()=>-3;try{process.kill(7,0)}catch(error){result.push(['errno',error.name,error.code,error.message,error.errno,error.syscall])}
process._kill=original;console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn process_signals_preserve_delivery_order_listener_lifetime_and_uncaught_identity() {
    let source = r#"const result=[];
for(const name of ['SIGTERM','SIGINT','SIGHUP'])process.once(name,function(signal){result.push([signal,this===process])});
for(const name of ['SIGHUP','SIGTERM','SIGINT'])process.kill(process.pid,name);
process.nextTick(()=>result.push(['tick']));Promise.resolve().then(()=>result.push(['promise']));
for(const name of ['SIGKILL','SIGSTOP']){try{process.on(name,()=>{})}catch(error){result.push([name,error.name,error.code,error.errno,error.syscall,error.message])}}
const marker={};process.on('uncaughtException',error=>result.push(['uncaught',error===marker]));
process.once('SIGWINCH',()=>{throw marker});process.kill(process.pid,'SIGWINCH');
setImmediate(()=>{result.push(['immediate']);console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn exit_from_timer_abandons_remaining_callbacks_without_uncaught_exception() {
    let source = r#"process.on('uncaughtException',()=>console.log('unexpected uncaught'));
process.on('exit',code=>console.log('exit',code));
setTimeout(()=>process.exit(0),0);setTimeout(()=>console.log('unexpected timer'),5);"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0));
        assert!(observation.stderr.is_empty());
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
#[cfg(unix)]
fn stdin_reads_pipe_bytes_and_releases_input_at_end() {
    let source = r#"const pieces=[];let buffers=true;const stream=process.stdin;
console.log('identity',stream===process.stdin,stream.fd,stream.readable,typeof stream.resume,typeof stream.pause);
stream.on('data',chunk=>{buffers&&=Buffer.isBuffer(chunk);pieces.push(chunk)});
stream.on('end',()=>console.log('input',buffers,Buffer.concat(pieces).toString('utf8')));"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        std::io::Write::write_all(&mut writer, "hello 😀\nworld".as_bytes()).unwrap();
        drop(writer);
        let input: std::os::fd::OwnedFd = reader.into();
        command.stdin(std::process::Stdio::from(input));
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0));
        assert!(observation.stderr.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
#[cfg(unix)]
fn cli_stdin_preserves_source_arguments_and_commonjs_origin() {
    let source = "console.log(JSON.stringify([process.argv.slice(1),process.execArgv,__filename,__dirname,module.id,require('path').basename(module.filename)]));";
    for arguments in [vec![], vec!["-", "--argument"]] {
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.args(&arguments);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
            std::io::Write::write_all(&mut writer, source.as_bytes()).unwrap();
            drop(writer);
            let input: std::os::fd::OwnedFd = reader.into();
            command.stdin(std::process::Stdio::from(input));
            quench_node_test::case_process::observe_command(command, Duration::from_secs(10))
                .unwrap()
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
}

#[test]
#[cfg(unix)]
fn open_stdin_repeats_public_getter_and_preserves_resume_receiver() {
    let source = r#"const trace=[];const second={};const first={resume(){trace.push(this===first)}};let reads=0;
Object.defineProperty(process,'stdin',{configurable:true,get(){trace.push('get');return ++reads===1?first:second}});
console.log(JSON.stringify([process.openStdin.call(null)===second,process.openStdin.name,process.openStdin.length,trace]));"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0));
        assert!(observation.stderr.is_empty());
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
#[cfg(unix)]
fn child_stdio_specs_match_node_pipe_ignore_inherit_and_descriptors() {
    let source = r#"const {spawn}=require('child_process');const results=[];
const cases=[['pipe','pipe','pipe'],['inherit','pipe','ignore'],[0,'pipe','ignore'],[{fd:0},'pipe','ignore'],'ignore'];
let index=0;function next(){if(index===cases.length){console.log(JSON.stringify(results));return;}
const stdio=cases[index++];const child=spawn(process.execPath,['-e',`let input='';process.stdin.setEncoding('utf8');process.stdin.on('data',chunk=>input+=chunk);process.stdin.on('end',()=>console.log('input:'+input));`],{stdio});
const record=[child.stdin===null,child.stdout===null,child.stderr===null,child.stdio.map(stream=>stream===null)];let output='';
child.stdout?.on('data',chunk=>output+=chunk);if(child.stdin)child.stdin.end('piped');
child.on('error',error=>{throw error});child.on('close',(code,signal)=>{record.push(code,signal,output);results.push(record);next()});}next();
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let (reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        std::io::Write::write_all(&mut writer, b"inherited").unwrap();
        drop(writer);
        let input: std::os::fd::OwnedFd = reader.into();
        command.stdin(std::process::Stdio::from(input));
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn stream_defaults_match_node_mutation_descriptors_and_validation() {
    let source = r#"
const s = require('stream');
function snapshot() {
  console.log(JSON.stringify([s.getDefaultHighWaterMark(), s.getDefaultHighWaterMark(true),
    new s.Readable({read(){}}).readableHighWaterMark,
    new s.Writable({write(c,e,cb){cb()}}).writableHighWaterMark]));
}
snapshot();
for (const [mode,value] of [[false,1024],[true,7],[false,0]]) {
  s.setDefaultHighWaterMark(mode,value); snapshot();
}
for (const value of [undefined,null,'2',1.5,-1,Infinity,NaN]) {
  try { s.setDefaultHighWaterMark(false,value); console.log('accepted'); }
  catch (e) { console.log(JSON.stringify([e.name,e.code,e.message])); }
}
for (const name of ['getDefaultHighWaterMark','setDefaultHighWaterMark']) {
  const f=s[name],d=Object.getOwnPropertyDescriptor(s,name);
  console.log(JSON.stringify([f.name,f.length,!!f.prototype,d.writable,d.enumerable,d.configurable]));
}
"#;
    compare_node_stream_source(source);
}

#[test]
fn bounded_zlib_streams_match_node_roundtrips_and_close_lifecycle() {
    let source = r#"
const z=require('zlib');
const input=Buffer.from('hello bounded codecs '.repeat(4096));
const modes=[['gzip','gunzip'],['deflate','inflate'],['deflateRaw','inflateRaw'],
  ['brotliCompress','brotliDecompress'],['zstdCompress','zstdDecompress']];
function next() {
  if (!modes.length) return closeStream();
  const [compress,decompress]=modes.shift();
  const encoded=z[compress+'Sync'](input);
  const decoder=z['create'+decompress[0].toUpperCase()+decompress.slice(1)]();
  const chunks=[];
  decoder.on('data',chunk=>chunks.push(chunk));
  decoder.on('end',()=>{console.log(decompress,Buffer.concat(chunks).equals(input));next();});
  decoder.end(encoded);
}
function closeStream() {
const closed=z.createGzip();
closed.on('data',()=>closed.close());
closed.on('close',()=>console.log('closed',closed.destroyed));
closed.end(input);
}
next();
"#;
    compare_node_stream_source(source);
}

fn compare_node_stream_source(source: &str) -> [quench_node_test::case_process::CaseObservation; 2] {
    compare_node_source_with_args(source, &[])
}

fn compare_node_source_with_args(source: &str, flags: &[&str]) -> [quench_node_test::case_process::CaseObservation; 2] {
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(flags).args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(
            observation.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
    observations
}

#[test]
fn native_stat_watch_cache_stop_order_and_reference_counts_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-statwatch-ref-')),file=path.join(dir,'watched');fs.writeFileSync(file,'data');
const a=()=>assert.fail('unchanged file'),b=()=>assert.fail('unchanged file');
const watcher=fs.watchFile(file,{interval:5},a);assert.strictEqual(fs.watchFile(path.join(dir,'.','watched'),{interval:'ignored'},b),watcher);
assert.strictEqual(watcher.unref(),watcher);assert.strictEqual(watcher.unref(),watcher);assert.strictEqual(watcher.ref(),watcher);assert.strictEqual(watcher.ref(),watcher);assert.strictEqual(watcher.ref(),watcher);
fs.unwatchFile(file,()=>{});assert.strictEqual(watcher.listenerCount('change'),2);fs.unwatchFile(file,a);assert.strictEqual(watcher.listenerCount('change'),1);
const result=[];let synchronous=true;watcher.once('stop',()=>{assert(!synchronous);result.push('stop');});watcher.stop();assert.strictEqual(watcher._handle,null);watcher.stop();assert.strictEqual(fs.watchFile(file,b),watcher);fs.unwatchFile(file);synchronous=false;
process.nextTick(()=>{assert.deepStrictEqual(result,['stop']);fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));});
"#);
}

#[test]
fn native_stat_watch_missing_creation_stats_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-statwatch-stats-')),file=path.join(dir,'watched');
const options=Object.freeze({interval:0});let phase=0;const result=[];
const watcher=fs.watchFile(file,options,(current,previous)=>{
 if(phase++===0){assert.strictEqual(current.ino,0);assert.strictEqual(previous.ino,0);assert.strictEqual(current.atime.getTime(),0);assert.strictEqual(current.isFile(),false);assert(current instanceof fs.Stats);result.push(['missing',Object.keys(current).filter(x=>!x.endsWith('time')),Object.getOwnPropertySymbols(current).map(String)]);fs.writeFileSync(file,'abc');}
 else {assert(current.ino>0);assert.strictEqual(previous.ino,0);assert.strictEqual(current.size,3);assert(current.isFile());assert(!current.isDirectory());result.push(['created',current.size,current.isFile()]);fs.unwatchFile(file);}
});watcher.on('stop',()=>{result.push('stop');fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));});
"#);
}

#[test]
fn native_stat_watch_initial_request_survives_unreference_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs');
fs.watchFile('quench-statwatch-initial-missing',{interval:1,persistent:false},()=>console.log('initial'));
console.log('ready');
"#);
}

#[test]
fn native_stat_watch_stats_constructor_projection_matches_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),assert=require('assert');process.noDeprecation=true;
console.log(fs.Stats.name,fs.Stats.length,fs.Stats.prototype.constructor.name,Object.getPrototypeOf(fs.Stats).name);
const events=[];const sec={valueOf(){events.push('seconds');return 1;}};const ns={valueOf(){events.push('nanoseconds');return 234567890;}};
const stats=new fs.Stats(1,32768,1,2,3,0,4096,4,5,6,sec,ns,0,0,0,0,0,0);
assert(stats instanceof fs.Stats);assert.strictEqual(stats.isFile(),true);assert.strictEqual(stats.isDirectory(),false);assert.strictEqual(stats.atimeMs,1234.56789);assert.strictEqual(stats.atime.getTime(),1235);assert(Object.hasOwn(stats,'atime'));
stats._checkModeProperty=function(value){assert.strictEqual(this,stats);return 'override:'+value;};assert.strictEqual(stats.isFile(),'override:32768');
console.log(JSON.stringify([events,Object.keys(stats),Object.getOwnPropertySymbols(stats).map(String)]));
"#);
}

#[test]
fn native_recursive_watch_class_close_and_per_path_hooks_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-recwatch-hooks-')),sub=path.join(dir,'sub'),file=path.join(sub,'file');fs.mkdirSync(sub);fs.writeFileSync(file,'before');
const original=fs.watch,hooks=[],references=[];
fs.symlinkSync(file,path.join(dir,'link'));
fs.watch=function(filename,options,listener){const name=path.relative(dir,filename);hooks.push(name);const child=Reflect.apply(original,this,[filename,options,listener]);for(const key of ['ref','unref']){const method=child[key];child[key]=function(){references.push([name,key]);return Reflect.apply(method,this,[]);};}return child;};
const watcher=original(dir,{recursive:true});fs.watch=original;
const result=[['paths',hooks],['meta',watcher.constructor.name,Object.keys(watcher),Object.getOwnPropertySymbols(Object.getPrototypeOf(watcher)).map(String),['close','ref','unref'].map(k=>[watcher[k].name,watcher[k].length,Object.hasOwn(watcher[k],'prototype')])]];
assert.strictEqual(watcher.ref(),undefined);assert.strictEqual(watcher.unref(),undefined);result.push(['references',references]);
let synchronous=true,closed=0;watcher.on('close',()=>{assert(synchronous);closed++;result.push('close');});
watcher.close();synchronous=false;watcher.close();assert.strictEqual(closed,1);fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));
"#);
}

#[test]
fn native_recursive_watch_discovery_changes_and_ignore_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-recwatch-events-')),sub=path.join(dir,'sub'),file=path.join(sub,'visible');fs.mkdirSync(sub);fs.writeFileSync(file,'before');
const watcher=fs.watch(dir,{recursive:true,ignore:['*.log',/\.tmp$/]});const result=[];let stage=0;
watcher.on('change',(kind,name)=>{assert(!name.endsWith('.log'));assert(!name.endsWith('.tmp'));if(stage===0&&name==='sub/visible'){assert.strictEqual(kind,'change');stage++;result.push([kind,name]);fs.mkdirSync(path.join(dir,'new'));fs.writeFileSync(path.join(dir,'new','added'),'new');}
else if(stage===1&&name==='new/added'){assert.strictEqual(kind,'rename');stage++;result.push([kind,name]);watcher.close();}});
watcher.once('close',()=>{assert.strictEqual(stage,2);fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));});
fs.writeFileSync(path.join(sub,'ignored.log'),'log');fs.writeFileSync(path.join(sub,'ignored.tmp'),'tmp');fs.writeFileSync(file,'after');
"#);
}

#[test]
fn native_recursive_watch_validation_matches_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-recwatch-options-')),result=[];
for(const options of [{recursive:'1'},{recursive:1},{recursive:true,persistent:1},{recursive:true,throwIfNoEntry:'1'}]){
try{fs.watch(dir,options).close();assert.fail('expected error');}catch(error){result.push([error.name,error.code,error.message]);}}
const watcher=fs.watch(dir,{recursive:true,persistent:false,encoding:'buffer'});assert.strictEqual(watcher.ref(),undefined);assert.strictEqual(watcher.unref(),undefined);watcher.close();
const missing=path.join(dir,'missing');for(const throwIfNoEntry of [undefined,true,false]){try{const absent=fs.watch(missing,{recursive:true,throwIfNoEntry});assert.strictEqual(throwIfNoEntry,false);absent.close();result.push(['missing','ignored']);}catch(error){assert.notStrictEqual(throwIfNoEntry,false);assert.strictEqual(error.code,'ENOENT');assert.strictEqual(error.filename,missing);result.push(['missing',error.name,error.code]);}}
fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));
"#);
}

#[test]
fn native_recursive_watch_globstar_root_and_nested_paths_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
for(const file of ['node_modules','node_modules/package.json','a/node_modules','a/node_modules/package.json','src/app.js'])console.log(file,path.matchesGlob(file,'**/node_modules'),path.matchesGlob(file,'**/node_modules/**'));
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-recwatch-glob-'));fs.mkdirSync(path.join(dir,'node_modules'));fs.mkdirSync(path.join(dir,'src'));
const watcher=fs.watch(dir,{recursive:true,ignore:['**/node_modules/**','**/node_modules']});watcher.on('change',(kind,name)=>{assert(!name.includes('node_modules'));if(name==='src/app.js'){assert.strictEqual(kind,'rename');watcher.close();}});
watcher.once('close',()=>{fs.rmSync(dir,{recursive:true,force:true});console.log('globstar');});fs.writeFileSync(path.join(dir,'node_modules','package.json'),'{}');fs.writeFileSync(path.join(dir,'src','app.js'),'code');
"#);
}

#[test]
fn native_recursive_watch_callback_rm_is_async_and_preserves_errors_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-rm-callback-')),file=path.join(dir,'file');fs.writeFileSync(file,'data');let synchronous=true;
fs.rm(file,error=>{assert(!synchronous);assert.strictEqual(error,null);assert(!fs.existsSync(file));fs.rm(file,(error)=>{assert.strictEqual(error.code,'ENOENT');fs.rm(file,{force:true},(error)=>{assert.strictEqual(error,null);fs.rmSync(dir,{recursive:true,force:true});console.log('callback-rm');});});});synchronous=false;
"#);
}

#[test]
fn native_recursive_watch_promises_forward_iterator_and_abort_cause_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),fsp=require('fs/promises'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-recwatch-promises-')),result=[];
(async()=>{const ac=new AbortController(),marker={original:true};const aborted=fsp.watch(dir,{recursive:true,signal:ac.signal});const pending=aborted.next();ac.abort(marker);await assert.rejects(pending,error=>error.code==='ABORT_ERR'&&error.cause===marker);result.push('abort');
const pre=fsp.watch(dir,{recursive:true,signal:AbortSignal.abort(marker)});await assert.rejects(pre.next(),error=>error.code==='ABORT_ERR'&&error.cause===marker);result.push('preabort');
const watcher=fsp.watch(dir,{recursive:true});const next=watcher.next();process.nextTick(()=>fs.writeFileSync(path.join(dir,'file'),'data'));const value=await next;assert.strictEqual(value.done,false);assert.strictEqual(value.value.eventType,'rename');assert.strictEqual(value.value.filename,'file');assert.strictEqual((await watcher.return()).done,true);assert.strictEqual((await watcher.next()).done,true);fs.rmSync(dir,{recursive:true,force:true});result.push('event');console.log(JSON.stringify(result));})();
"#);
}

#[test]
fn native_recursive_watch_internal_url_conversion_preserves_brands_and_identity() {
    const SOURCE: &str = r#"
const assert=require('assert'),url=require('url'),internal=require('internal/url');
assert.strictEqual(require('internal/url'),internal);assert.strictEqual(internal.URL,url.URL);assert.strictEqual(internal.fileURLToPath,url.fileURLToPath);assert.strictEqual(Object.hasOwn(url,'toPathIfFileURL'),false);
for(const value of ['.',undefined,null,Buffer.from('path'),{href:'file:///tmp/path'}])assert.strictEqual(internal.toPathIfFileURL(value),value);
assert.strictEqual(internal.toPathIfFileURL(new URL('file:///tmp/a%20b')),'/tmp/a b');
try{internal.toPathIfFileURL(new URL('https://example.com/'));assert.fail('expected error');}catch(error){console.log(error.code);}
console.log('internal-url');
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-internals", "-e", SOURCE]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(), "{}", String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn native_promise_watch_buffers_burst_events_and_closes_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),fsp=require('fs/promises'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-pwatch-burst-'));
(async()=>{const watcher=fsp.watch(dir);const next=watcher.next();process.nextTick(()=>{for(const name of ['a','b','c'])fs.writeFileSync(path.join(dir,name),'data');});
const first=await next;assert.strictEqual(first.done,false);assert.strictEqual(Object.getPrototypeOf(first.value),null);const seen=new Set([first.value.filename]);await new Promise(resolve=>setTimeout(resolve,30));
while(seen.size<3){const item=await watcher.next();seen.add(item.value.filename);}assert.deepStrictEqual([...seen].sort(),['a','b','c']);assert.strictEqual((await watcher.return()).done,true);assert.strictEqual((await watcher.next()).done,true);fs.rmSync(dir,{recursive:true,force:true});console.log('buffered');})();
"#);
}

#[test]
fn native_promise_watch_lazy_validation_matches_node() {
    compare_node_stream_source(r#"
const fs=require('fs/promises'),assert=require('assert');
(async()=>{const result=[];
for(const [file,options] of [[1,{}],['.',1],['.',null],['.',[]],['.',{persistent:1}],['.',{recursive:1}],['.',{encoding:1}],['.',{signal:1}],['.',{maxQueue:'silly'}],['.',{maxQueue:1.5}],['.',{maxQueue:Infinity}],['.',{maxQueue:9007199254740992}],['.',{overflow:1}],['.',{overflow:'bad'}],['.',{ignore:123}],['.',{ignore:''}]]){
let iterator;assert.doesNotThrow(()=>{iterator=fs.watch(file,options);});try{await iterator.next();assert.fail('expected error');}catch(error){result.push([error.name,error.code,error.message]);}}
console.log(JSON.stringify(result));})();
"#);
}

#[test]
fn native_promise_watch_abort_preserves_cause_and_descriptor_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),fsp=require('fs/promises'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-pwatch-abort-'));
(async()=>{const result=[];for(const pre of [false,true]){const ac=new AbortController(),marker={original:true};if(pre)ac.abort(marker);const watcher=fsp.watch(dir,{signal:ac.signal});const pending=watcher.next();if(!pre)ac.abort(marker);
try{await pending;assert.fail('expected abort')}catch(error){assert.strictEqual(error.cause,marker);assert.strictEqual(error.code,'ABORT_ERR');const d=Object.getOwnPropertyDescriptor(error,'cause');result.push([pre,error.name,error.constructor.name,Object.keys(error),[d.enumerable,d.writable,d.configurable]]);}}
fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));})();
"#);
}

#[test]
fn native_promise_watch_overflow_error_releases_handle_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),fsp=require('fs/promises'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-pwatch-overflow-'));
(async()=>{const watcher=fsp.watch(dir,{maxQueue:1,overflow:'error'});const first=watcher.next();process.nextTick(()=>fs.writeFileSync(path.join(dir,'initial'),'data'));await first;
for(const name of ['a','b','c','d'])fs.writeFileSync(path.join(dir,name),'data');await new Promise(resolve=>setTimeout(resolve,50));
await assert.rejects(watcher.next(),error=>{assert.strictEqual(error.code,'ERR_FS_WATCH_QUEUE_OVERFLOW');console.log(error.message);return true;});assert.strictEqual((await watcher.next()).done,true);fs.rmSync(dir,{recursive:true,force:true});console.log('overflow-closed');})();
"#);
}

#[test]
fn native_promise_watch_abort_resists_stopped_propagation_like_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),fsp=require('fs/promises'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-pwatch-stopabort-'));
(async()=>{const ac=new AbortController(),marker={original:true};ac.signal.addEventListener('abort',event=>event.stopImmediatePropagation());const watcher=fsp.watch(dir,{signal:ac.signal});const next=watcher.next();ac.abort(marker);await assert.rejects(next,error=>error.code==='ABORT_ERR'&&error.cause===marker);
const other=new AbortController();other.signal.addEventListener('abort',event=>event.stopImmediatePropagation());const publicWatcher=fs.watch(dir,{signal:other.signal});let closed=false;publicWatcher.once('close',()=>{closed=true;});other.abort();await new Promise(resolve=>process.nextTick(resolve));assert(closed);fs.rmSync(dir,{recursive:true,force:true});console.log('resisted');})();
"#);
}

#[test]
fn module_path_to_file_url_uses_guest_cwd_and_original_resolver_hooks() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert'),{pathToFileURL}=require('url');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-file-url-cwd-')),before=process.cwd();
fs.mkdirSync(path.join(dir,'test'),{recursive:true});fs.mkdirSync(path.join(dir,'tests/node/test'),{recursive:true});fs.writeFileSync(path.join(dir,'test/target'),'ordinary');fs.writeFileSync(path.join(dir,'tests/node/test/target'),'other');process.chdir(dir);
assert.strictEqual(pathToFileURL('./test/target').pathname,dir+'/test/target');assert.strictEqual(pathToFileURL('./test/').pathname,dir+'/test/');
const resolve=path.posix.resolve,marker={};path.posix.resolve=function(value){assert.strictEqual(this,path.posix);assert.strictEqual(value,'input/');return '/hook/dir';};assert.strictEqual(pathToFileURL('input/').href,'file:///hook/dir/');path.posix.resolve=()=>{throw marker};assert.throws(()=>pathToFileURL('input'),e=>e===marker);path.posix.resolve=resolve;
assert.throws(()=>pathToFileURL('input',{get windows(){throw marker}}),e=>e===marker);process.chdir(before);fs.rmSync(dir,{recursive:true,force:true});console.log('cwd-and-resolver');
"#);
}

#[test]
fn module_node_search_paths_share_resolution_and_preserve_public_hooks() {
    compare_node_stream_source(r#"
const Module=require('module'),path=require('path'),assert=require('assert');
for(const input of ['/','/node_modules','/a/node_modules/b/node_modules/c','/a/foo_node_modules','/a/../b/./c','','.','..'])console.log(JSON.stringify([input,Module._nodeModulePaths(input)]));
console.log(JSON.stringify([Module._nodeModulePaths.name,Module._nodeModulePaths.length,Object.getOwnPropertyDescriptor(Module._nodeModulePaths,'name')]));
console.log(JSON.stringify(Reflect.ownKeys(Module._nodeModulePaths).map(String)));
console.log(JSON.stringify(new Module._nodeModulePaths('/construct/node_modules/child')));
for(const value of [undefined,null,1,{},Symbol('x')])try{Module._nodeModulePaths(value)}catch(e){console.log(JSON.stringify([e.name,e.code,e.message]));}
const original=path.resolve,marker={};path.resolve=function(...args){assert.strictEqual(this,path);console.log('resolve-hook',args.length);return '/hook/node_modules/child';};console.log(JSON.stringify(Module._nodeModulePaths(marker)));path.resolve=()=>{throw marker};assert.throws(()=>Module._nodeModulePaths('/absolute'),e=>e===marker);path.resolve=original;
"#);
}

#[test]
fn module_create_require_url_shapes_errors_and_directory_base_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const {createRequire}=require('module'),{pathToFileURL}=require('url');
console.log(JSON.stringify([Reflect.ownKeys(createRequire).map(String),Object.getOwnPropertyDescriptor(createRequire,'prototype').writable]));
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-create-require-'));
fs.mkdirSync(path.join(dir,'新建文件夹'));fs.writeFileSync(path.join(dir,'新建文件夹','dependency.cjs'),'module.exports=42');
const file=path.join(dir,'新建文件夹','あ.js'),url=pathToFileURL(file),directory=path.dirname(file)+path.sep;
for(const value of [file,url,url.href,pathToFileURL(directory),directory]){const req=createRequire(value);assert.strictEqual(req('./dependency.cjs'),42);assert.strictEqual(req.resolve('./dependency.cjs'),path.join(directory,'dependency.cjs'));}
for(const value of [undefined,null,1,{},'../','https://example.org/x','file://remote/x','file:///a%2Fb','😀'.repeat(100)]){
 try{createRequire(value);assert.fail('accepted invalid value')}catch(e){console.log(JSON.stringify([e.name,e.code,e.message,Object.keys(e),Object.getOwnPropertyDescriptor(e,'message'),e.constructor.name,Reflect.ownKeys(Object.getPrototypeOf(e)).map(String)]));}
}
const log=[],candidate={get href(){log.push('href');return 'file:///irrelevant'},get protocol(){log.push('protocol');return 'file:'},get auth(){log.push('auth')},get path(){log.push('path')},toString(){log.push('toString');return pathToFileURL(directory).href}};
assert.strictEqual(createRequire(candidate)('./dependency.cjs'),42);console.log(JSON.stringify(log));
const marker={};const originalURL=global.URL;global.URL=function(){throw marker};assert.strictEqual(createRequire(url)('./dependency.cjs'),42);global.URL=originalURL;assert.strictEqual((new createRequire(url))('./dependency.cjs'),42);assert.throws(()=>createRequire({get href(){throw marker}}),e=>e===marker);
const coercion={href:'x',protocol:'file:',toString(){throw marker}};assert.throws(()=>createRequire(coercion),e=>e.code==='ERR_INVALID_ARG_VALUE'&&e!==marker);
console.log('url-and-directory');fs.rmSync(dir,{recursive:true,force:true});
"#);
}

#[test]
fn commonjs_namespace_static_prefix_uses_syntax_and_matches_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-cjs-export-prefix-'));
const sources={accessor:"const refresh=()=>1,resolve=()=>2;module.exports={refresh,resolve,get path(){return '.';}};",nested:"const before=1,after=2;module.exports={before,nested:{x:1},after};",literal:"const later=2;module.exports={first:1,later};",assignments:"const text='exports.fake=1';/* exports.comment=1 */module.exports['hyphen-name']=3;module.exports.for=4;function unused(){exports.never=5;} exports.compound+=1;exports.logical ||=3;"};
const expressions=['value','value.x','value["x"]','value?.x','fn()','fn+1','true','null','undefined','this','new Date()','function(){}','class{}','async()=>1','/x/','`x`','"text"','1','-1','(value)','\\u0076alue','π'];
const properties=[...expressions.map(value=>'b:'+value),'...value','...require("./spread.cjs")','...value.x','b(){}','async b(){}','get b(){return 1}','set b(x){}','*b(){}','"b"(){}','["b"]:value','b:value /* hi */','b:π','b:v\\u0061lue'];
fs.writeFileSync(path.join(dir,'spread.cjs'),'exports.spread=1');
for(const [index,property] of properties.entries()) sources['prefix-'+index]='const value={x:1},fn=()=>1,π=value;module.exports={a:value,'+property+',c:value}';
(async()=>{for(const [name,source] of Object.entries(sources)){const file=path.join(dir,name+'.cjs');fs.writeFileSync(file,source);const namespace=await import(file);assert.strictEqual(namespace.default,require(file));assert.strictEqual(namespace['module.exports'],namespace.default);console.log(name,Object.keys(namespace).join(','));if(name==='accessor'){assert.strictEqual(namespace.refresh(),1);assert.strictEqual(namespace.resolve(),2);}if(name==='assignments'){assert(!Object.hasOwn(namespace,'fake'));assert(!Object.hasOwn(namespace,'comment'));assert.strictEqual(namespace['hyphen-name'],3);assert.strictEqual(namespace.for,4);}}fs.rmSync(dir,{recursive:true,force:true});})();
"#);
}

#[test]
fn commonjs_namespace_follows_static_require_reexports_and_cycles() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-cjs-reexports-'));
fs.writeFileSync(path.join(dir,'leaf.cjs'), 'exports.answer=42; exports.forwardRef=x=>x;');
fs.writeFileSync(path.join(dir,'index.cjs'), "module.exports = require('./bridge.cjs');");
fs.writeFileSync(path.join(dir,'bridge.cjs'), "module.exports = require('./leaf.cjs');");
fs.writeFileSync(path.join(dir,'cycle-a.cjs'), "exports.a=1;if(false)module.exports=require('./cycle-b.cjs');");
fs.writeFileSync(path.join(dir,'cycle-b.cjs'), "exports.b=2;if(false)module.exports=require('./cycle-a.cjs');");
(async()=>{
  const file=path.join(dir,'index.cjs'), ns=await import(file);
  assert.strictEqual(ns.default,require(file));
  assert.strictEqual(ns.answer,42); assert.strictEqual(ns.forwardRef(7),7);
  console.log(Object.keys(ns).join(','));
  const cycle=await import(path.join(dir,'cycle-a.cjs'));
  console.log(Object.keys(cycle).join(','));
  fs.rmSync(dir,{recursive:true,force:true});
})();
"#);
}

#[test]
fn global_crypto_is_lazy_and_random_values_preserve_views() {
    compare_node_stream_source(r#"
const assert=require('assert');
const descriptor=Object.getOwnPropertyDescriptor(globalThis,'crypto');
console.log(JSON.stringify([descriptor.enumerable,descriptor.configurable,typeof descriptor.get,descriptor.get.name,descriptor.get.length,descriptor.set]));
const cryptoObject=globalThis.crypto;
assert.strictEqual(cryptoObject,require('crypto').webcrypto);
const values=new Uint8Array(12), slice=values.subarray(3,9);
assert.strictEqual(cryptoObject.getRandomValues(slice),slice);
assert.strictEqual(values[0],0);assert.strictEqual(values[11],0);
for(const value of [new Float32Array(1),new DataView(new ArrayBuffer(1)),new Uint8Array(65537)]) {
  try{cryptoObject.getRandomValues(value);assert.fail('accepted invalid view');}
  catch(error){console.log(error.name);}
}
const method=cryptoObject.getRandomValues;
assert.throws(()=>method(new Uint8Array(1)), {code:'ERR_INVALID_THIS'});
console.log('random-views');
"#);
}

#[test]
fn buffer_alloc_supports_construction_and_ordinary_calls() {
    compare_node_stream_source(r#"
const assert=require('assert');
assert.strictEqual(Buffer.alloc.length,3);
for(const value of [Buffer.alloc(5,'ab'),new Buffer.alloc(5,'ab'),Reflect.construct(Buffer.alloc,[5,'ab'],function Other(){})]) {
  assert(Buffer.isBuffer(value));assert.strictEqual(value.toString(),'ababa');
  assert.strictEqual(Object.getPrototypeOf(value),Buffer.prototype);
}
assert.throws(()=>new Buffer.alloc(-1),{code:'ERR_OUT_OF_RANGE'});
console.log('constructible-alloc');
"#);
}

#[test]
fn http_agent_pool_names_preserve_property_reads_and_coercion() {
    compare_node_stream_source(r#"
const agent=new(require('http').Agent)();
for(const options of [undefined,{}, {host:'a',port:80},{host:'a',port:0,localAddress:'b',family:4,socketPath:'/tmp/socket'},{family:'4'},{family:6}]) console.log(agent.getName(options));
const log=[];
const value={ [Symbol.toPrimitive](hint){log.push(hint);return 'custom';} };
const options={get host(){log.push('host');return value},get port(){log.push('port');return value},get localAddress(){log.push('localAddress');return value},get family(){log.push('family');return 6},get socketPath(){log.push('socketPath');return value}};
console.log(agent.getName(options));console.log(JSON.stringify(log));
agent.destroy();
"#);
}

#[test]
fn extensionless_entries_use_the_package_module_scope() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("package.json"), r#"{"type":"module"}"#).unwrap();
    std::fs::write(directory.path().join("dependency.js"), "export default 42;").unwrap();
    let entry = directory.path().join("cli");
    std::fs::write(&entry, "import value from './dependency.js';console.log(value);").unwrap();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.arg(&entry);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(result.stdout, b"42\n"); assert!(result.stderr.is_empty());
    }
}

#[test]
fn imported_strict_terminal_calls_preserve_local_exception_handlers() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("library.mjs"), r#"
function bad() { throw Error('marker'); }
export function caught() { try { return bad(); } catch (error) { return error.message; } }
export function viaParameter(fn) { try { return fn(); } catch (error) { return error.message; } }
export function nested() { try { return bad(); } catch { try { return bad(); } catch { return null; } } }
export function finalized(events) { try { return bad(); } catch { return 42; } finally { events.push('finally'); } }
export { bad };
"#).unwrap();
    let entry = directory.path().join("entry.mjs");
    std::fs::write(&entry, r#"
import { caught, viaParameter, nested, finalized, bad } from './library.mjs';
const events=[];
console.log(JSON.stringify([caught(),viaParameter(bad),nested(),finalized(events),events]));
"#).unwrap();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.arg(&entry);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(result.stdout, b"[\"marker\",\"marker\",null,42,[\"finally\"]]\n");
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn structured_clone_preserves_array_elements_holes_and_aliases() {
    compare_node_stream_source(r#"
const assert=require('node:assert');
assert.deepStrictEqual(structuredClone([2]),[2]);
const shared={answer:42}, source=[]; source.length=5000;
source[1]=shared; source[4096]=shared; source.extra=shared; source.self=source;
let reads=0;
Object.defineProperty(source,'3',{enumerable:true,get(){reads++;delete source[4];return shared}});
source[4]='deleted by getter';
Object.defineProperty(source,'5',{value:'hidden',enumerable:false});
source[Symbol('hidden')]='symbol';
const prototype=Object.create(Array.prototype);
Object.defineProperty(prototype,'0',{get(){throw Error('inherited index was read')}});
Object.setPrototypeOf(source,prototype);
const clone=structuredClone(source);
assert.strictEqual(reads,1); assert.strictEqual(clone.length,5000);
assert.strictEqual(Object.getPrototypeOf(clone),Array.prototype);
for(const index of [0,4,5,4999]) assert.strictEqual(Object.hasOwn(clone,index),false);
assert.strictEqual(clone[1],clone[3]); assert.strictEqual(clone[1],clone[4096]);
assert.strictEqual(clone[1],clone.extra); assert.strictEqual(clone.self,clone);
assert.notStrictEqual(clone[1],shared);
assert.deepStrictEqual(Object.getOwnPropertyDescriptor(clone,'1'),{value:clone[1],writable:true,enumerable:true,configurable:true});
console.log(JSON.stringify([Object.keys(clone),clone.length,clone[1],reads]));
"#);
}

#[test]
fn structured_clone_errors_preserve_intrinsics_descriptors_snapshots_and_cycles() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),types=require('node:util').types,out=[];
for(const Type of [Error,EvalError,RangeError,ReferenceError,SyntaxError,TypeError,URIError,AggregateError]) {
 const source=Type===AggregateError?new Type([], 'message'):new Type('message');
 source.stack='saved stack';source.extra=42;source.cause=source;
 const copy=structuredClone([source,source]),clone=copy[0];
 assert.strictEqual(clone,copy[1]);assert.strictEqual(clone.cause,clone);assert.notStrictEqual(clone,source);
 assert.strictEqual(Object.getPrototypeOf(clone),Type===AggregateError?Error.prototype:Type.prototype);
 assert.strictEqual(types.isNativeError(clone),true);assert.strictEqual(clone.stack,'saved stack');
 assert.deepStrictEqual(Object.keys(clone),[]);assert.strictEqual(Object.hasOwn(clone,'extra'),false);
 assert.deepStrictEqual(Object.getOwnPropertyDescriptor(clone,'message'),{value:'message',writable:true,enumerable:false,configurable:true});
 const descriptor=Object.getOwnPropertyDescriptor(clone,'cause');assert.strictEqual(descriptor.enumerable,false);assert.strictEqual(descriptor.writable,true);assert.strictEqual(descriptor.configurable,true);
 const stack=Object.getOwnPropertyDescriptor(clone,'stack');out.push([clone.name,typeof stack.get,typeof stack.set,stack.enumerable,stack.configurable]);
 clone.stack='changed';assert.strictEqual(clone.stack,'changed');
}

const unicodeError=new Error();unicodeError.message={toString(){return '\ud800message\udfff'}};unicodeError.stack='fixed';assert.strictEqual(structuredClone(unicodeError).message,'\ud800message\udfff');
const trace=[],oldCause={old:true},source=new Error();
Object.defineProperties(source,{
 message:{configurable:true,value:{toString(){trace.push('message');return 'snapshot message'}}},
 cause:{configurable:true,value:oldCause},
 name:{get(){trace.push('name');Object.defineProperty(source,'message',{value:'mutated'});Object.defineProperty(source,'cause',{value:null});const garbage=[];for(let i=0;i<20000;i++)garbage.push({i});return {toString(){trace.push('name coercion');return 'TypeError'}}}},
 stack:{configurable:true,get(){trace.push('stack');return 'snapshot stack'}}
});
const clone=structuredClone(source);assert.strictEqual(clone.message,'snapshot message');assert.deepStrictEqual(clone.cause,{old:true});assert.notStrictEqual(clone.cause,oldCause);assert.strictEqual(Object.getPrototypeOf(clone),TypeError.prototype);assert.deepStrictEqual(trace,['name','name coercion','message','stack']);out.push(trace);
for(const stack of [undefined,42,null,{}]){const source=new Error();Object.defineProperty(source,'stack',{value:stack});Object.defineProperty(source,'message',{get(){throw 'message accessor'}});Object.defineProperty(source,'cause',{get(){throw 'cause accessor'}});const c=structuredClone(source);assert.strictEqual(Object.hasOwn(c,'message'),false);assert.strictEqual(Object.hasOwn(c,'cause'),false);assert.strictEqual(c.stack,undefined);}
for(const property of ['name','stack']){const marker={};const e=new Error();Object.defineProperty(e,property,{get(){throw marker}});assert.throws(()=>structuredClone(e),e=>e===marker);}
for(const property of ['name','message']){const e=new Error();Object.defineProperty(e,'stack',{value:'fixed'});Object.defineProperty(e,property,{value:Symbol('bad')});assert.throws(()=>structuredClone(e),TypeError);}
const original=TypeError,e=new original('intrinsic');e.stack='fixed';globalThis.TypeError=function(){throw 'replacement'};const c=structuredClone(e);assert.strictEqual(Object.getPrototypeOf(c),original.prototype);globalThis.TypeError=original;
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn structured_clone_builtin_graphs_preserve_snapshots_and_ignore_public_hooks() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),out=[];
const key={key:1},map=new Map(),set=new Set();map.set(key,set);map.set(map,map);set.add(key);set.add(map);set.add(set);
for(const value of [map,set]){Object.defineProperty(value,Symbol.iterator,{get(){throw 'iterator'}});Object.defineProperty(value,'extra',{enumerable:true,get(){throw 'extra'}});Object.setPrototypeOf(value,Object.create(Object.getPrototypeOf(value)));}
const oldSet=Map.prototype.set,oldAdd=Set.prototype.add;Map.prototype.set=Set.prototype.add=function(){throw 'public insertion'};
const graph=structuredClone([map,set,key,map]);Map.prototype.set=oldSet;Set.prototype.add=oldAdd;
const [m,s,k]=graph;assert.strictEqual(graph[3],m);assert.strictEqual(m.get(k),s);assert.strictEqual(m.get(m),m);assert.deepStrictEqual([...s],[k,m,s]);assert.strictEqual(Object.getPrototypeOf(m),Map.prototype);assert.strictEqual(Object.getPrototypeOf(s),Set.prototype);assert.deepStrictEqual(Object.keys(m),[]);
const snapshot=new Map(),first={get value(){snapshot.clear();return 'first'}},last={last:true};snapshot.set(first,last);snapshot.set(last,first);const copied=structuredClone(snapshot);const pairs=[...copied];assert.strictEqual(pairs.length,2);assert.strictEqual(pairs[0][0],pairs[1][1]);assert.strictEqual(pairs[0][1],pairs[1][0]);assert.strictEqual(pairs[0][0].value,'first');
const members=new Set(),element={get value(){members.clear();members.add('late');return 'first'}};members.add(element);members.add(last);assert.deepStrictEqual([...structuredClone(members)],[{value:'first'},{last:true}]);
for(const time of [0,-1,1700000000123,NaN]){const date=new Date(time);Object.defineProperty(date,'getTime',{get(){throw 'date hook'}});Object.defineProperty(date,'extra',{enumerable:true,get(){throw 'date extra'}});const pair=structuredClone([date,date]);assert.strictEqual(pair[0],pair[1]);assert(Object.is(pair[0].getTime(),time));assert.strictEqual(Object.getPrototypeOf(pair[0]),Date.prototype);assert.deepStrictEqual(Object.keys(pair[0]),[]);}
const regexp=/a(b)/dgimsuy;regexp.lastIndex=99;for(const name of ['source','flags','extra'])Object.defineProperty(regexp,name,{enumerable:true,get(){throw 'regexp hook'}});const pair=structuredClone([regexp,regexp]);assert.strictEqual(pair[0],pair[1]);assert.strictEqual(pair[0].source,'a(b)');assert.strictEqual(pair[0].flags,'dgimsuy');assert.strictEqual(pair[0].lastIndex,0);assert.strictEqual(Object.getPrototypeOf(pair[0]),RegExp.prototype);assert.deepStrictEqual(Object.keys(pair[0]),[]);
for(const [name,value] of [['Map',new Map([[1,2]])],['Set',new Set([1])],['Date',new Date(42)],['RegExp',/x/g]]){const original=globalThis[name],prototype=original.prototype;globalThis[name]=function(){throw 'public constructor'};const clone=structuredClone(value);assert.strictEqual(Object.getPrototypeOf(clone),prototype);globalThis[name]=original;out.push([name,Object.prototype.toString.call(clone)]);}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn cyclic_esm_instantiates_live_bindings_before_running_dependencies() {
    for (fixture, expected) in [("esm-graph", "cyclic-esm-live-bindings\n"), ("esm-errors", "esm-link-error-retry\n")] {
    let entry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/npm-top300/regressions/{fixture}/entry.mjs"));
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.arg(&entry);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(result.stdout, expected.as_bytes());
        assert!(result.stderr.is_empty());
    }
    }
}

#[test]
fn stack_locations_use_module_urls_and_commonjs_paths() {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("entry space # ü.mjs");
    let helper = directory.path().join("capture.cjs");
    std::fs::write(&helper, r#"
exports.capture = function capture() {
  const previous = Error.prepareStackTrace;
  Error.prepareStackTrace = (_, sites) => sites;
  const target = {};
  Error.captureStackTrace(target);
  const sites = target.stack;
  Error.prepareStackTrace = previous;
  return sites;
};
"#).unwrap();
    std::fs::write(&entry, r#"
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import helper from './capture.cjs';
function caller() { return helper.capture(); }
const sites = caller();
const own = sites.find(site => site.getFunctionName() === 'caller');
const common = sites.find(site => site.getFunctionName() === 'capture');
assert.equal(own.getFileName(), import.meta.url);
assert.equal(common.getFileName(), fileURLToPath(new URL('./capture.cjs', import.meta.url)));
assert(own.toString().includes(import.meta.url));
assert(common.toString().includes(common.getFileName()));
console.log('stack-source-identities');
"#).unwrap();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.arg(&entry);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(result.stdout, b"stack-source-identities\n");
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn console_formats_module_values_without_a_global_require() {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("entry.mjs");
    std::fs::write(&entry, r#"
import util from 'node:util';
util.format=()=>{throw Error('public formatter replacement was used')};
util.formatWithOptions=util.format;
console.log(null,undefined,42);
console.log('%s:%d:%j','answer',42,{value:null});
console.log(null,undefined);
"#).unwrap();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.arg(&entry);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(result.stdout, b"null undefined 42\nanswer:42:{\"value\":null}\nnull undefined\n");
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn json_import_attributes_parse_the_original_payload() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-json-module-'));
const file=path.join(dir,'value.json');fs.writeFileSync(file,'{"value":[1,null,"hello"]}');
(async()=>{const ns=await import(file,{with:{type:'json'}});assert.deepStrictEqual(ns.default,{value:[1,null,'hello']});console.log(Object.keys(ns).join(','));
fs.writeFileSync(path.join(dir,'bad.json'),']');try{await import(path.join(dir,'bad.json'),{with:{type:'json'}});assert.fail('invalid JSON accepted')}catch(e){console.log(e.name)}
fs.rmSync(dir,{recursive:true,force:true});})();
"#);
}

#[test]
fn module_resolve_filename_uses_the_parent_base_without_loading() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert'),Module=require('module');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-parent-resolve-'));
fs.writeFileSync(path.join(dir,'dependency.cjs'),'throw Error("must not execute")');
const parent={id:path.join(dir,'noop.js'),filename:path.join(dir,'noop.js'),paths:Module._nodeModulePaths(dir)};
assert.strictEqual(Module._resolveFilename('./dependency.cjs',parent),path.join(dir,'dependency.cjs'));
assert.strictEqual(Module._resolveFilename('fs',parent),'fs');
assert.throws(()=>Module._resolveFilename('./missing.cjs',parent),{code:'MODULE_NOT_FOUND'});
console.log('parent-resolution');fs.rmSync(dir,{recursive:true,force:true});
"#);
}

#[test]
fn module_constructor_and_compile_preserve_receiver_exports_and_parent() {
    compare_node_stream_source(r#"
const assert=require('node:assert'),Module=require('node:module');
assert.strictEqual(typeof Module,'function');assert.strictEqual(Module.Module,Module);
assert.strictEqual(Module.length,0);assert.strictEqual(Module.name,'Module');
const parent={children:[]};const m=new Module('/a/b.js',parent);
assert(m instanceof Module);assert.strictEqual(m.constructor,Module);
assert.deepStrictEqual(Object.keys(m),['id','path','exports','filename','loaded','children']);
assert.strictEqual(m.id,'/a/b.js');assert.strictEqual(m.path,'/a');
assert.strictEqual(m.filename,null);assert.strictEqual(m.loaded,false);
assert.strictEqual(m.parent,parent);assert.strictEqual(parent.children[0],m);
const replacement={};m.parent=replacement;assert.strictEqual(m.parent,replacement);
assert.strictEqual(m.require('node:assert'),assert);
const result=m._compile('exports.value=42;exports.info=[__filename,__dirname,module.filename,module.loaded,this===exports,arguments.length];','/virtual/file.cjs');
assert.strictEqual(result,undefined);assert.deepStrictEqual(m.exports,{value:42,info:['/virtual/file.cjs','/virtual',null,false,true,5]});
assert.strictEqual(m.filename,null);assert.strictEqual(m.loaded,false);
m._compile('#!/usr/bin/node\nmodule.exports={replacement:true};','virtual.cjs');
assert.deepStrictEqual(m.exports,{replacement:true});
m._compile('\uFEFFmodule.exports={bom:true};','virtual.cjs');assert.deepStrictEqual(m.exports,{bom:true});
const getter=Object.getOwnPropertyDescriptor(Module.prototype,'constructor');
assert.strictEqual(typeof getter.get,'function');assert.strictEqual(getter.set,undefined);
assert.strictEqual(getter.enumerable,false);assert.strictEqual(getter.configurable,false);
assert.throws(()=>new Module(null),e=>e.code==='ERR_INVALID_ARG_TYPE');
assert.throws(()=>Module('bare'),TypeError);
console.log(JSON.stringify([m.id,m.path,m.loaded,m.exports,Module.prototype._compile.length]));
"#);
}

#[cfg(target_os = "linux")]
#[test]
fn os_cpus_reports_online_linux_facts_and_millisecond_times() {
    compare_node_stream_source(r#"
const assert=require('node:assert'),fs=require('node:fs'),os=require('node:os');
function snapshot(){return fs.readFileSync('/proc/stat','utf8').split('\n').filter(s=>/^cpu\d+ /.test(s)).map(s=>s.trim().split(/\s+/).slice(1).map(Number))}
const before=snapshot(),cpus=os.cpus(),after=snapshot();
assert.strictEqual(os.cpus.name,'cpus');assert.strictEqual(os.cpus.length,0);
assert.strictEqual(cpus.length,before.length);assert(Array.isArray(new os.cpus()));
for(let i=0;i<cpus.length;i++){
 const cpu=cpus[i];assert.deepStrictEqual(Object.keys(cpu),['model','speed','times']);
 assert.strictEqual(typeof cpu.model,'string');assert(Number.isInteger(cpu.speed)&&cpu.speed>=0);
 const keys=['user','nice','sys','idle','irq'],indices=[0,1,2,3,5];
 assert.deepStrictEqual(Object.keys(cpu.times),keys);
 for(let j=0;j<keys.length;j++){const value=cpu.times[keys[j]],index=indices[j];assert(Number.isInteger(value));assert(value>=before[i][index]*10&&value<=after[i][index]*10)}
}

console.log(JSON.stringify([cpus.length,cpus.map(cpu=>cpu.model)]));
"#);
}

#[test]
fn binary_base64_globals_preserve_utf16_coercion_validation_and_identity() {
    compare_node_stream_source(r#"
const assert=require('node:assert'),buffer=require('node:buffer');
assert.strictEqual(buffer.btoa,btoa);assert.strictEqual(buffer.atob,atob);
assert.strictEqual(btoa.name,'btoa');assert.strictEqual(atob.name,'atob');
assert.strictEqual(btoa.length,1);assert.strictEqual(atob.length,1);
const bytes=String.fromCharCode(...Array.from({length:256},(_,i)=>i));
assert.strictEqual(atob(btoa(bytes)),bytes);
assert.strictEqual(btoa(undefined),'dW5kZWZpbmVk');assert.strictEqual(btoa(null),'bnVsbA==');
for(const value of ['\ud800','\udfff','\u0100','💩'])assert.throws(()=>btoa(value),e=>e.name==='InvalidCharacterError'&&e.code===5&&e.message==='Invalid character');
for(const value of ['-A','_A','AA=','AAAA=','A===','AAAA\v','AAAA\u00a0'])assert.throws(()=>atob(value),e=>e.name==='InvalidCharacterError'&&e.code===5&&e.message==='Invalid character');
for(const value of ['A','AAAAA',undefined])assert.throws(()=>atob(value),e=>e.message==='The string to be decoded is not correctly encoded.');
assert.strictEqual(atob(' A A == \n\t\r\f'),'\0');assert.strictEqual(atob('AB'),'\0');
let calls=0;assert.strictEqual(btoa({[Symbol.toPrimitive](hint){assert.strictEqual(hint,'string');calls++;return 'A'}}),'QQ==');assert.strictEqual(calls,1);
assert.throws(()=>btoa(Symbol('x')),TypeError);assert.throws(()=>atob(Symbol('x')),TypeError);
assert.throws(()=>btoa(),e=>e.code==='ERR_MISSING_ARGS');assert.throws(()=>atob(),e=>e.code==='ERR_MISSING_ARGS');
assert(new btoa('A') instanceof btoa);assert(new atob('QQ==') instanceof atob);
const original=DOMException;globalThis.DOMException=function(){throw Error('replacement used')};
assert.throws(()=>atob('!'),e=>e instanceof original&&e.name==='InvalidCharacterError');globalThis.DOMException=original;
console.log(JSON.stringify([btoa(bytes),atob('SGVsbG8=')]));
"#);
}

#[test]
fn read_file_sync_accepts_file_urls_and_uses_url_validation() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert'),url=require('url');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-url-'));
const file=path.join(dir,'a space %.txt');fs.writeFileSync(file,'hello');
assert.strictEqual(fs.readFileSync(url.pathToFileURL(file),'utf8'),'hello');
for(const input of ['https://example.com/a','file://example.com/a','file:///a%2Fb']) {
  try { fs.readFileSync(new URL(input));assert.fail('invalid file URL accepted'); }
  catch(error) { console.log(error.name,error.code); }
}
console.log('file-url-read');fs.rmSync(dir,{recursive:true,force:true});
"#);
}

#[test]
fn public_commonjs_wrappers_have_five_parameters_and_no_internal_bindings() {
    compare_node_stream_source(r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-cjs-wrapper-'));
const file=path.join(dir,'module.cjs');
fs.writeFileSync(file,'const primordials=42;module.exports=[arguments.length,primordials,typeof internalBinding]');
assert.deepStrictEqual(require(file),[5,42,'undefined']);
console.log('public-cjs-wrapper');fs.rmSync(dir,{recursive:true,force:true});
"#);
}

#[test]
fn really_exit_coerces_int32_and_skips_guest_exit_handlers() {
    for value in ["undefined", "7.9", "'7'", "true", "NaN", "4294967298", "Symbol()", "1n", "({valueOf(){console.log('coerce');throw Error('marker')}})"] {
        let source = format!("process.on('exit',()=>console.log('must-not-emit'));console.log('before');process.reallyExit({value});console.log('must-not-return');");
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.args(["-e", &source]);
            if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
        });
        for observation in &observations {
            assert!(!observation.timed_out); assert!(observation.signal.is_none());
            assert!(observation.stderr.is_empty(), "{}", String::from_utf8_lossy(&observation.stderr));
        }
        assert_eq!(observations[0].exit_code, observations[1].exit_code, "{value}");
        assert_eq!(observations[0].stdout, observations[1].stdout, "{value}");
    }
}

#[test]
fn native_recursive_watch_iterator_and_gc_match_node() {
    const SOURCE: &str = r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-recwatch-iterator-')),file=path.join(dir,'file');fs.writeFileSync(file,'before');
(async()=>{const watcher=fs.watch(dir,{recursive:true});const iterator=watcher[Symbol.asyncIterator]();assert.strictEqual(iterator[Symbol.asyncIterator](),iterator);const pending=iterator.next();global.gc();fs.writeFileSync(file,'after');const value=await pending;assert.strictEqual(Object.getPrototypeOf(value),null);assert.strictEqual(value.value.eventType,'change');assert.strictEqual(value.value.filename,'file');assert.strictEqual(Object.hasOwn(value,'done'),false);const done=iterator.return();assert.strictEqual(done.done,true);assert.strictEqual(Object.getPrototypeOf(done),null);assert.strictEqual(iterator.next().done,true);global.gc();fs.rmSync(dir,{recursive:true,force:true});console.log('iterator-gc');})();
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", SOURCE]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(), "{}", String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn native_stat_watch_bigint_timestamps_and_original_callback_error_match_node() {
    const SOURCE: &str = r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-statwatch-bigint-')),file=path.join(dir,'watched'),result=[];let phase=0;const marker={original:true};
const watcher=fs.watchFile(file,{interval:1,bigint:true},(current,previous)=>{
 global.gc();assert.strictEqual(typeof current.ino,'bigint');assert.strictEqual(typeof previous.ino,'bigint');
 for(const key of ['atime','mtime','ctime','birthtime'])assert.strictEqual(current[key+'Ns']/1000000n,current[key+'Ms']);
 if(phase++===0){assert.strictEqual(current.ino,0n);assert.strictEqual(current.atime.getTime(),0);result.push('missing');fs.writeFileSync(file,'abc');fs.utimesSync(file,1.234567,1.234567);}
 else {assert(current.ino>0n);assert.strictEqual(current.size,3n);assert(current.isFile());assert(current.ctimeNs>9007199254740991n);assert.strictEqual(current.atime.getTime(),1234);result.push('created');fs.unwatchFile(file);throw marker;}
});
process.once('uncaughtException',error=>{assert.strictEqual(error,marker);result.push('original');});
watcher.once('stop',()=>{global.gc();fs.rmSync(dir,{recursive:true,force:true});result.push('stop');console.log(JSON.stringify(result));});global.gc();
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", SOURCE]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(), "{}", String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn native_stat_watch_validation_option_getters_and_stop_removal_match_node() {
    compare_node_stream_source(r#"
const fs=require('fs'),assert=require('assert'),path=require('path'),os=require('os');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-statwatch-validation-')),file=path.join(dir,'watched');fs.writeFileSync(file,'data');const result=[];
for(const value of [undefined,null,false,1,'bad',{},()=>{}]){try{fs.watchFile(file,value);result.push(['accepted',typeof value]);if(typeof value==='function')fs.unwatchFile(file);}catch(error){result.push([typeof value,error.name,error.code,error.message]);}}
for(const value of [-1,1.5,NaN,Infinity,4294967296,'1',null,undefined]){try{fs.watchFile(file,{interval:value},()=>{});assert.fail('expected interval error');}catch(error){result.push(['interval',String(value),error.name,error.code,error.message]);}}
const getters=[],symbol=Symbol('option');const options=Object.create({interval:1});Object.defineProperties(options,{interval:{enumerable:true,get(){getters.push('interval');return 0;}},persistent:{enumerable:true,get(){getters.push('persistent');return false;}},[symbol]:{enumerable:true,get(){getters.push('symbol');return 1;}}});
const watcher=fs.watchFile(file,options,()=>assert.fail('unchanged'));result.push(getters);const never=()=>assert.fail('removed stop listener');watcher.once('stop',never);watcher.stop();watcher.removeListener('stop',never);fs.unwatchFile(file);process.nextTick(()=>{fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));});
"#);
}

#[test]
fn zlib_crc32_and_bounded_sync_inputs_match_node_errors_and_bytes() {
    let source = r#"
const z=require('zlib');
for (const data of ['hello',Buffer.from([0,127,128,255]),new Uint16Array([256,513]),new DataView(new Uint8Array([8,9]).buffer)])
  for (const seed of [undefined,0,1,4294967295]) console.log(z.crc32(data,seed));
for (const data of [undefined,null,true,1,[],{},function named(){},new ArrayBuffer(2)])
  try{z.crc32(data)}catch(e){console.log(e.name,e.code,e.message)}
for (const seed of [null,true,{},'0',-1,1.5,Infinity,NaN,4294967296])
  try{z.crc32('hello',seed)}catch(e){console.log(e.name,e.code,e.message)}
console.log(z.crc32.name,z.crc32.length,!!z.crc32.prototype);
for (const data of [undefined,null,true,1,[],{},function named(){}])
  try{z.deflateSync(data)}catch(e){console.log(e.name,e.code,e.message)}
for (const data of [new Uint16Array([256,513]),new DataView(new Uint8Array([8,9]).buffer),new Uint8Array([1,2]).buffer])
  console.log(z.inflateSync(z.deflateSync(data)).toString('hex'));
const encoded=z.brotliCompressSync(Buffer.alloc(256));
for (const limit of [64,128.5,256,undefined,NaN,null,'64',0,Infinity])
  try{console.log('length',z.brotliDecompressSync(encoded,{maxOutputLength:limit}).length)}
  catch(e){console.log(e.name,e.code,e.message)}
console.log(z.gunzipSync(Buffer.concat([z.gzipSync('abc'),z.gzipSync('def')])).toString());
"#;
    compare_node_stream_source(source);
}

#[test]
fn zlib_callback_exceptions_escape_once_after_initial_jobs() {
    let source = r#"
const z=require('zlib');
let tick=false,promise=false,count=0;
const marker={};
process.nextTick(()=>tick=true);
Promise.resolve().then(()=>promise=true);
process.on('uncaughtException',error=>console.log('uncaught',error===marker,count));
z.gzip('callback',function(error,buffer){
  count++;console.log('done',error===null,z.gunzipSync(buffer).toString(),tick,promise,this===globalThis);
  throw marker;
});
"#;
    compare_node_stream_source(source);
}

#[test]
fn zlib_options_match_node_ranges_parameters_and_getter_order() {
    let source = r#"
const z=require('zlib');
const fractional=z.createDeflate({level:1.5,strategy:1.5});console.log(fractional._level,fractional._strategy);fractional.destroy();
for (const mode of ['Gzip','Gunzip','Deflate','InflateRaw']) {
  for (const name of ['windowBits','level','memLevel','strategy','chunkSize','flush','finishFlush','maxOutputLength','rejectGarbageAfterEnd','dictionary']) {
    for (const value of [undefined,NaN,null,'bad',-Infinity,0,1.5,10000]) {
      try { const engine=new z[mode]({[name]:value});engine.destroy();console.log(mode,name,String(value),'ok'); }
      catch(e){ console.log(mode,name,String(value),e.name,e.code,e.message); }
    }
  }
}
for (const mode of ['BrotliCompress','ZstdCompress']) {
  for (const params of [{'10000':0},{'0':0,'00':0},{'0':'bad'}]) {
    try{new z[mode]({params});console.log(mode,'ok')}
    catch(e){console.log(mode,e.name,e.code,e.message)}
  }
}
for (const mode of ['Gzip','Gunzip','BrotliCompress']) {
  const trace=[];
  const opts={};
  for(const name of ['windowBits','level','memLevel','strategy','params','dictionary','chunkSize','flush','finishFlush','maxOutputLength','rejectGarbageAfterEnd','info'])
    Object.defineProperty(opts,name,{get(){trace.push(name);return undefined}});
  const engine=new z[mode](opts);engine.destroy();console.log(mode,JSON.stringify(trace));
}
"#;
    compare_node_stream_source(source);
}

#[test]
fn zlib_info_returns_captured_engines_and_preserves_handle_lifecycle() {
    let source = r#"
const z=require('zlib');
const result=z.gzipSync('hello',{info:true});
console.log(Buffer.isBuffer(result.buffer),result.engine instanceof z.Gzip,result.engine.destroyed,result.engine._closed,result.engine.bytesWritten);
const trace=[];
const options={get level(){trace.push('level');return 1},get maxOutputLength(){trace.push('max');return 1000},info:true};
z.gzip('hello',options,function callback(error,result){
  console.log(error===null,Buffer.isBuffer(result.buffer),result.engine===this,this.destroyed,this._closed,this.cb===callback,z.gunzipSync(result.buffer).toString(),JSON.stringify(trace));
});
options.info=false;
"#;
    compare_node_stream_source(source);
}

#[test]
#[cfg(unix)]
fn child_ipc_stdio_maps_a_real_duplex_socket_at_standard_and_extra_descriptors() {
    let source = r#"const {spawn}=require('child_process');let index=0;const results=[];
const slots=[0,3,5];function next(){if(index===slots.length){console.log(JSON.stringify(results));return;}
const slot=slots[index++];const stdio=['ignore','pipe','ignore'];while(stdio.length<=slot)stdio.push(null);stdio[slot]='ipc';
const child=spawn(process.argv[1],['-e',`console.log(JSON.stringify([process.channel.fd,require('fs').fstatSync(process.channel.fd).isSocket(),process.env.NODE_CHANNEL_FD===undefined]));`],{stdio});
let output='';child.stdout.on('data',chunk=>output+=chunk);child.on('error',error=>{throw error});child.on('close',(code,signal)=>{results.push([child.stdio.map(stream=>stream===null),code,signal,output]);next()});}next();"#;
    let node = Command::new("node")
        .arg("-p")
        .arg("process.execPath")
        .output()
        .unwrap();
    assert!(node.status.success());
    let node = String::from_utf8(node.stdout).unwrap();
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source, node.trim()]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
#[cfg(unix)]
fn fork_json_ipc_matches_node_frames_callbacks_and_disconnect_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let child = directory.path().join("ipc-child.cjs");
    std::fs::write(&child, r#"const stringify=JSON.stringify;JSON.parse=()=>{throw Error('guest parse')};JSON.stringify=()=>{throw Error('guest stringify')};
process.on('message',message=>{process.send({text:message.text,length:message.big.length,tail:message.big.slice(-2)});process.disconnect();});"#).unwrap();
    let source = r#"const {fork}=require('child_process');const stringify=JSON.stringify;const events=[];
const options={silent:true};const child=fork(process.argv[1],[],options);
events.push(['initial',child.connected,child.stdio.map(stream=>stream===null),Object.hasOwn(options,'stdio')]);
const control=child.channel,prototype=Object.getPrototypeOf(control),fdDescriptor=Object.getOwnPropertyDescriptor(prototype,'fd');
events.push(['shape',child.send.name,child.send.length,!!child.send.prototype,child.disconnect.name,child.disconnect.length,!!child.disconnect.prototype,
prototype.constructor.name,prototype.constructor.length,control instanceof require('events').EventEmitter,
control.ref.name,control.ref.length,!!control.ref.prototype,fdDescriptor.get.name,fdDescriptor.get.length,fdDescriptor.enumerable,fdDescriptor.configurable]);
child.stderr.on('data',chunk=>process.stderr.write(chunk));
for(const message of [undefined,Symbol('x'),()=>{}]){try{child.send(message)}catch(error){events.push(['error',error.name,error.code,error.message])}}
const circular={};circular.self=circular;try{child.send(circular)}catch(error){events.push(['circular',error.name])}
JSON.parse=()=>{throw Error('guest parse')};JSON.stringify=()=>{throw Error('guest stringify')};
child.on('message',message=>events.push(['message',message]));child.on('disconnect',()=>events.push(['disconnect',child.connected,child.channel===null,control.fd]));
child.on('close',(code,signal)=>{events.push(['close',code,signal]);child.send('closed',error=>{events.push(['closed-send',error.name,error.code,error.message]);console.log(stringify(events))})});
child.send({toJSON(){events.push(['toJSON']);return {text:'hé😀\n',big:'x'.repeat(180000)+'尾😀'}}},error=>events.push(['sent',error]));
process.nextTick(()=>events.push(['tick']));"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source, child.to_str().unwrap()]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
    });
    for observation in &observations {
        assert!(
            !observation.timed_out,
            "{}",
            String::from_utf8_lossy(&observation.stdout)
        );
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
#[cfg(unix)]
fn filesystem_descriptors_match_node_positioned_io_and_spawn_inheritance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("descriptor.txt");
    let source = r#"const fs=require('fs'),{spawn}=require('child_process');const path=process.argv[1];
fs.writeFileSync(path,'abcdef');const fd=fs.openSync(path,'r+');const bytes=Buffer.alloc(6,45);
const read=fs.readSync(fd,bytes,2,3,1);const written=fs.writeSync(fd,Buffer.from('XY'),1,1,3);
console.log(JSON.stringify([read,bytes.toString(),written,fs.readFileSync(path,'utf8'),fs.fstatSync(fd).isFile()]));
fs.writeSync(1,'descriptor-stdout\n');const child=spawn(process.execPath,['-e',`const fs=require('fs'),buffer=Buffer.alloc(3);const n=fs.readSync(0,buffer,0,buffer.length,null);console.log(JSON.stringify([n,buffer.toString()]));`],{stdio:[fd,'pipe','ignore']});
let output='';child.stdout.on('data',chunk=>output+=chunk);child.on('error',error=>{throw error});child.on('close',(code,signal)=>{fs.closeSync(fd);console.log(JSON.stringify([child.stdin===null,code,signal,output]));});"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source, path.to_str().unwrap()]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn node_entry_resolution_matches_extensionless_files_and_package_directories() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("entry.js"),
        "console.log(__filename.slice(__dirname.length+1));",
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("package")).unwrap();
    std::fs::write(
        directory.path().join("package/package.json"),
        r#"{"main":"main.cjs"}"#,
    )
    .unwrap();
    std::fs::write(
        directory.path().join("package/main.cjs"),
        "console.log(__filename.slice(__dirname.length+1));",
    )
    .unwrap();
    for entry in ["entry", "package"] {
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.current_dir(directory.path()).arg(entry);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap()
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
}

#[test]
fn readable_pipe_matches_node_stdio_lifetime_and_captured_end_option() {
    let source=r#"const {Readable,Writable}=require('stream');const events=[];
const stdoutEnd=process.stdout.end,stdoutDestroy=process.stdout.destroy;
process.stdout.end=()=>events.push('stdout-end');process.stdout.destroy=()=>events.push('stdout-destroy');
const stderrEnd=process.stderr.end,stderrDestroy=process.stderr.destroy;
process.stderr.end=()=>events.push('stderr-end');process.stderr.destroy=()=>events.push('stderr-destroy');
const out=Readable.from(['stdio-pipe\n']),err=Readable.from(['stderr-pipe\n']);out.pipe(process.stdout);err.pipe(process.stderr);
let content='';const target=new Writable({autoDestroy:false,write(chunk,encoding,callback){content+=chunk;callback()}});
const options={get end(){events.push('option');return false}};const input=Readable.from(['kept']);input.pipe(target,options);
Object.defineProperty(options,'end',{value:true});
setImmediate(()=>setImmediate(()=>{events.push(['target',content,target.writableEnded,target.destroyed]);
process.stdout.end=stdoutEnd;process.stdout.destroy=stdoutDestroy;process.stderr.end=stderrEnd;process.stderr.destroy=stderrDestroy;
console.log(JSON.stringify(events));}));"#;
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.args(["-e",source]);
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1");}
        quench_node_test::case_process::observe_command(command,Duration::from_secs(5)).unwrap()
    });
    for observation in &observations{assert!(!observation.timed_out);assert_eq!(observation.exit_code,Some(0),"{}",String::from_utf8_lossy(&observation.stderr));}
    assert_eq!(observations[0].stdout,observations[1].stdout);
    assert_eq!(observations[0].stderr,observations[1].stderr);
}

#[test]
fn child_stdin_backpressure_preserves_bytes_and_finishes_synchronously_drained_tail() {
    const SOURCE: &str = r#"
const assert=require('assert');
const child=require('child_process').spawn(process.execPath,['-e',`let n=0;process.stdin.on('data',b=>n+=b.length);process.stdin.on('end',()=>console.log(n));`]);
let output='', callbacks=0, finished=false;
child.stdout.on('data',chunk=>output+=chunk);
assert.strictEqual(child.stdin.write(Buffer.alloc(1024*1024),()=>callbacks++),false);
assert(child.stdin.writableLength>0);
for(let i=0;i<10;i++)child.stdin.write(Buffer.alloc(1),()=>callbacks++);
child.stdin.end(()=>finished=true);
child.on('close',(code)=>{assert.strictEqual(code,0);assert.strictEqual(callbacks,11);assert(finished);assert.strictEqual(output,'1048586\n');console.log('complete');});
"#;
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(10))
                .unwrap();
        assert!(
            !observation.timed_out,
            "{executable}: {}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{executable}: {}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert_eq!(observation.stdout, b"complete\n");
        assert!(observation.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn child_stdio_stream_forwarding_matches_node_pipe_ownership() {
    const SOURCES: [&str; 2] = [
        r#"
const assert=require('assert'),{spawn}=require('child_process');
const sink=spawn('cat',{stdio:['pipe','pipe','inherit']});
const first=spawn('cat',{stdio:['pipe',sink.stdin,'inherit']});
const second=spawn('cat',{stdio:['pipe',sink.stdin,'inherit']});
let output='';sink.stdout.setEncoding('utf8');
sink.stdout.once('data',chunk=>{assert.strictEqual(chunk,'first\n');output+=chunk;
second.stdin.end('second\n');sink.stdout.once('data',chunk=>{assert.strictEqual(chunk,'second\n');output+=chunk;
sink.stdin.end('direct\n');sink.stdout.once('data',chunk=>{assert.strictEqual(chunk,'direct\n');output+=chunk;});});});
sink.on('close',code=>{assert.strictEqual(code,0);assert.strictEqual(output,'first\nsecond\ndirect\n');console.log('complete');});
first.stdin.end('first\n');
"#,
        r#"
const assert=require('assert'),{spawn}=require('child_process');
const source=spawn('cat',{stdio:['pipe','pipe','inherit']});
const head=spawn('head',['-n1'],{stdio:[source.stdout,'pipe','inherit']});
let first='',rest='';head.stdout.setEncoding('utf8');head.stdout.on('data',chunk=>first+=chunk);
head.on('exit',()=>{source.stdout.setEncoding('utf8');source.stdout.on('data',chunk=>rest+=chunk);source.stdin.end('rest\n');source.stdout.resume();});
source.on('close',code=>{assert.strictEqual(code,0);assert.strictEqual(first,'first\n');assert.strictEqual(rest,'rest\n');console.log('complete');});
source.stdin.write('first\n');
"#,
    ];
    for source in SOURCES {
        for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
            let mut command = Command::new(executable);
            command.args(["-e", source]);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            let observation =
                quench_node_test::case_process::observe_command(command, Duration::from_secs(10))
                    .unwrap();
            assert!(
                !observation.timed_out,
                "{executable}: {}",
                String::from_utf8_lossy(&observation.stderr)
            );
            assert_eq!(
                observation.exit_code,
                Some(0),
                "{executable}: {}",
                String::from_utf8_lossy(&observation.stderr)
            );
            assert_eq!(observation.stdout, b"complete\n");
            assert!(observation.stderr.is_empty());
        }
    }
}

#[test]
fn legacy_pipe_close_and_throwing_completion_match_node_without_destroying_readables() {
    const SOURCE: &str = r#"
const {Stream,Readable}=require('stream');
const observations=[];
for(const modern of [false,true])for(const event of ['end','close']){
 const source=modern?new Readable({read(){}}):new Stream();source.readable=true;
 const destination=new Stream();destination.writable=true;destination.write=()=>true;
 const calls=[];destination.end=()=>calls.push('end');destination.destroy=()=>calls.push('destroy');
 source.pipe(destination);const before=[source.listeners('end').length,source.listeners('close').length];
 source.emit(event);source.emit('close');
 observations.push([modern,event,calls,before,source.listeners('end').length,source.listeners('close').length]);
}
for(const event of ['end','close']){
 const source=new Stream(),destination=new Stream(),marker={event};let calls=0;
 source.readable=true;destination.writable=true;destination.write=()=>true;
 destination.end=destination.destroy=()=>{calls++;throw marker};source.pipe(destination);
 let same=false;try{source.emit(event)}catch(error){same=error===marker}
 source.emit('close');observations.push([event,same,calls,source.listeners('close').length]);
}
let gets=0;const source=new Stream(),destination=new Stream();source.readable=true;destination.writable=true;
 destination._isStdio=true;destination.write=()=>true;destination.end=destination.destroy=()=>{throw Error('stdio completion')};
 source.pipe(destination,{get end(){gets++;return true}});source.emit('end');source.emit('close');
observations.push([gets,process.stdout._isStdio,process.stderr._isStdio]);
console.log(JSON.stringify(observations));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn events_listener_count_preserves_target_branding_and_method_dispatch() {
    const SOURCE: &str = r#"
const {listenerCount}=require('events');const result=[];
const key=Symbol.for('nodejs.event_target');
const target=new EventTarget(), fn=()=>{};
target.addEventListener('x',fn);target.addEventListener('x',fn);
target.addEventListener('x',()=>{}, {once:true});
result.push(listenerCount(target,'x'));target.dispatchEvent(new Event('x'));
result.push(listenerCount(target,'x'));target.removeEventListener('x',fn);
result.push(listenerCount(target,'x'));
target.addEventListener('0',fn);result.push(listenerCount(target,0),listenerCount(target,'0'));
class Derived extends EventTarget {}result.push(listenerCount(new Derived(),'x'));
result.push(listenerCount(Object.create(EventTarget.prototype),'x'));
result.push(listenerCount({constructor:{[key]:true}},'x'));
const controller=new AbortController();controller.signal.addEventListener('abort',fn);
result.push(controller.signal instanceof EventTarget,listenerCount(controller.signal,'abort'));
controller.signal.removeEventListener('abort',fn);result.push(listenerCount(controller.signal,'abort'));
for(const onabort of [fn,null,{},fn]){controller.signal.onabort=onabort;result.push(listenerCount(controller.signal,'abort'));}
let gets=0;const delegated={get listenerCount(){gets++;return function(event){return [this===delegated,event,arguments.length]}}};
result.push(listenerCount(delegated,'y'),gets);
const marker={};try{listenerCount({get listenerCount(){throw marker}},'x')}catch(error){result.push(error===marker)}
for(const value of [{},null,undefined])try{listenerCount(value,'x')}catch(error){result.push([error.name,error.code??null])}
target.constructor={};try{listenerCount(target,'x')}catch(error){result.push([error.name,error.code])}
const constructor=EventTarget;globalThis.EventTarget=function(){};
result.push(listenerCount(new constructor(),'x'));
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn child_environment_coercion_inheritance_and_undefined_match_node() {
    const SOURCE: &str = r#"
const {spawn,spawnSync}=require('child_process');const trace=[];
const env=Object.create({INHERITED:9,MASKED:'prototype'});
Object.defineProperty(env,'MASKED',{value:'hidden',enumerable:false});
Object.assign(env,{NUMBER:42,BOOL:false,NULL:null,UNDEFINED:undefined,EMPTY:'',
 OBJECT:{toString(){trace.push('coerce');return 'object-value'}}});
Object.defineProperty(env,'GETTER',{enumerable:true,get(){trace.push('get');return 7}});
env[Symbol('ignored')]='symbol';
const childSource="console.log(JSON.stringify(['NUMBER','BOOL','NULL','UNDEFINED','EMPTY','OBJECT','GETTER','INHERITED','MASKED'].map(key=>[key,process.env[key]??null])))";
const sync=spawnSync(process.execPath,['-e',childSource],{env});
const result=[sync.status,sync.stdout.toString().trim(),trace.slice()];
for(const value of ['x\0y',Symbol('value')]){
 try{spawnSync(process.execPath,['-e',''],{env:{VALUE:value}})}
 catch(error){result.push([error.name,error.code??null,error.message])}
}
const marker={};try{spawnSync(process.execPath,['-e',''],{env:{get VALUE(){throw marker}}})}
catch(error){result.push(error===marker)}
trace.length=0;const child=spawn(process.execPath,['-e',childSource],{env});let output='';
child.stdout.on('data',chunk=>output+=chunk);child.on('error',error=>{throw error});
child.on('close',code=>{result.push(code,output.trim(),trace.slice());console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn tcp_reuse_port_shares_os_listeners_across_processes_like_node() {
    const SOURCE: &str = r#"
const assert=require('assert'),net=require('net'),cp=require('child_process');
const hosts=['127.0.0.1','::'];const result=[];
function next(){if(!hosts.length){console.log(JSON.stringify(result));return}
 const host=hosts.shift();const first=net.createServer();
 first.listen({host,port:0,reusePort:true},()=>{
  const port=first.address().port;const second=net.createServer();
  second.listen({host,port,reusePort:true},()=>{
   assert.strictEqual(second.address().port,port);
   const source="const net=require('net');const server=net.createServer();server.listen({host:process.env.HOST,port:+process.env.PORT,reusePort:true},()=>server.close(()=>console.log('bound')));";
   const child=cp.spawnSync(process.execPath,['-e',source],{env:{...process.env,HOST:host,PORT:port}});
   assert.strictEqual(child.status,0,child.stderr.toString());
   assert.strictEqual(child.stdout.toString(),'bound\n');
   result.push([first.address().family,second.address().family,true]);
   second.close(()=>first.close(next));
  });
 });
}next();
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn child_stdout_without_newline_is_visible_before_stdin_response() {
    const SOURCE: &str = r#"
const {spawn}=require('child_process');const assert=require('assert');
const source="const timer=setInterval(()=>{},1000);process.stdin.on('data',chunk=>{clearInterval(timer);console.log('ack:'+chunk.toString())});process.stdout.write('ready');";
const child=spawn(process.execPath,['-e',source]);let output='',replied=false;
child.stdout.on('data',chunk=>{output+=chunk;if(!replied&&output==='ready'){replied=true;assert.strictEqual(child.kill(0),true);child.stdin.end('input')}});
child.on('error',error=>{throw error});child.on('close',(code,signal)=>{assert.strictEqual(code,0);assert.strictEqual(signal,null);assert.strictEqual(replied,true);assert.strictEqual(output,'readyack:input\n');console.log('complete')});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert_eq!(observation.stdout, b"complete\n");
    }
}

#[test]
fn emitter_inspection_unwraps_once_and_preserves_raw_copies_and_filters() {
    const SOURCE: &str = r#"
const EE=require('events');const result=[],emitter=new EE();const symbol=Symbol('event');
const first=()=>{},second=()=>{};
emitter.on('__proto__',first);emitter.on('toString',second);emitter.on(symbol,first);
result.push(emitter.eventNames().map(key=>typeof key==='symbol'?'symbol':key));
emitter.on('event',first);emitter.once('event',first);emitter.once('event',second);
result.push(emitter.listeners('event').map(fn=>fn===first?'first':'second'));
const raw=emitter.rawListeners('event');result.push(raw[0]===first,raw[1]!==first,raw[1].listener===first,raw[2].listener===second);
raw.length=0;result.push(emitter.listenerCount('event'),emitter.listenerCount('event',first),emitter.listenerCount('event',second),emitter.listenerCount('event',null));
emitter.emit('event');result.push(emitter.listenerCount('event'),emitter.listenerCount('event',first));
emitter.removeAllListeners();result.push(emitter.eventNames().length,emitter.listeners(symbol).length);
emitter.on(undefined,first);result.push(emitter.listeners(undefined)[0]===first,emitter.listenerCount(undefined));
let monitors=0;emitter.once(EE.errorMonitor,()=>monitors++);emitter.on('error',()=>{});
emitter.emit('error',new Error('first'));emitter.emit('error',new Error('second'));
result.push(monitors,emitter.listenerCount(EE.errorMonitor));
const listeners=emitter.listeners;emitter.listeners=()=>{throw Error('public override')};
result.push(emitter.listenerCount(undefined),listeners.call(emitter,undefined)[0]===first);
for(const name of ['eventNames','listeners','rawListeners','listenerCount']){
 const fn=EE.prototype[name],descriptor=Object.getOwnPropertyDescriptor(EE.prototype,name);
 result.push([fn.name,fn.length,descriptor.writable,descriptor.enumerable,descriptor.configurable]);
 try{fn.call(null,'event')}catch(error){result.push(error.name)}
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn event_listener_limits_share_defaults_and_match_target_symbols_and_errors() {
    const SOURCE: &str = r#"
const EE=require('events'),{getMaxListeners,setMaxListeners}=EE;const result=[];
const emitter=new EE(),target=new EventTarget(),signal=new AbortController().signal;
result.push(EE.defaultMaxListeners,emitter.getMaxListeners(),getMaxListeners(emitter),getMaxListeners(target),getMaxListeners(signal));
EE.defaultMaxListeners=7;
result.push(new EE().getMaxListeners(),getMaxListeners(new EventTarget()),getMaxListeners(target),getMaxListeners(new AbortController().signal));
result.push(emitter.setMaxListeners(Infinity)===emitter,String(getMaxListeners(emitter)));
result.push(setMaxListeners(3,emitter,target,signal)===undefined,getMaxListeners(emitter),getMaxListeners(target),getMaxListeners(signal));
setMaxListeners(5);result.push(EE.defaultMaxListeners,new EE().getMaxListeners(),getMaxListeners(new EventTarget()),getMaxListeners(new AbortController().signal));
const fake={getMaxListeners(){throw Error('must not delegate')},_maxListeners:13};result.push(getMaxListeners(fake));
const key=EE.kMaxEventTargetListeners;result.push(typeof key,Symbol.keyFor(key)===undefined,getMaxListeners({[key]:42}));
const marker={},branded={constructor:{[Symbol.for('nodejs.event_target')]:true}};setMaxListeners(9,branded);result.push(getMaxListeners(branded));
for(const value of [NaN,-1,-4294967297,undefined,null,'x',{},false,0n]){
 for(const action of [()=>emitter.setMaxListeners(value),()=>{EE.defaultMaxListeners=value}]){
  try{action();result.push('accepted')}catch(error){result.push([error.name,error.code,error.message])}
 }
}
for(const value of [undefined,null,1,{}]){try{getMaxListeners(value)}catch(error){result.push([error.name,error.code])}}
for(const name of ['defaultMaxListeners','kMaxEventTargetListeners','kMaxEventTargetListenersWarned']){
 const descriptor=Object.getOwnPropertyDescriptor(EE,name);result.push([name,descriptor.enumerable,descriptor.configurable,descriptor.writable??null]);
}
let gets=0,calls=[];const custom={get setMaxListeners(){gets++;return function(n){calls.push([this===custom,n])}}};setMaxListeners(11,custom);result.push(gets,calls);
const throws={get setMaxListeners(){throw marker}};try{setMaxListeners(11,throws)}catch(error){result.push(error===marker)}
for(const target of [Object.freeze(new EE()),Object.freeze(new EventTarget()),new Proxy(new EventTarget(),{set(){return false}})]){
 try{setMaxListeners(7,target);result.push('accepted')}catch(error){result.push(error.name)}
}

console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn event_listener_warnings_preserve_error_identity_types_and_once_delivery() {
    const SOURCE: &str = r#"
const EE=require('events');const result=[],warnings=[];
const supplied=new Error('supplied');supplied.tag=7;
const emitter=new EE();emitter.setMaxListeners(1);const type=Symbol('event');
process.on('warning',warning=>{
 if(warning===supplied){warnings.push(['supplied',warning.tag]);return}
 warnings.push([warning.name,warning.emitter===emitter,warning.type===type,warning.count,warning.message]);
});
const OriginalError=globalThis.Error;globalThis.Error=class Replacement{};process.emitWarning(supplied);globalThis.Error=OriginalError;
emitter.on(type,()=>{});emitter.on(type,()=>{});result.push(emitter._events[type].warned);
emitter.on(type,()=>{});result.push(emitter._events[type].warned,warnings.length);
setImmediate(()=>{result.push(warnings);console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--no-warnings", "-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn warning_options_validation_and_deprecation_delivery_match_node() {
    const SOURCE: &str = r#"
const result=[],warnings=[];let thrown;
process.on('warning',warning=>warnings.push([warning.name,warning.message,warning.code??null,warning.detail??null]));
const supplied=new Error('deferred');supplied.name='DeprecationWarning';
process.on('uncaughtException',error=>{result.push(['thrown',error===supplied]);process.throwDeprecation=false});
for(const [warning,type,code] of [[undefined,undefined,undefined],[{},undefined,undefined],['x',null,undefined],['x',1,undefined],['x',[],undefined],['x','Warning',3],[new Error('x'),'Warning',3],['x',{type:3},undefined]]){
 try{process.emitWarning(warning,type,code);result.push('accepted')}catch(error){result.push([error.name,error.code,error.message])}
}
const getters=[];process.emitWarning('object',{get ctor(){getters.push('ctor')},get code(){getters.push('code');return 'CODE'},get detail(){getters.push('detail');return 'detail'},get type(){getters.push('type');return 'Custom'}});
result.push(getters);
process.emitWarning('function',function CustomName(){});
process.emitWarning('empty',{type:''});
process.noDeprecation=true;process.emitWarning('ignored','DeprecationWarning',1);process.emitWarning(supplied);process.noDeprecation=false;
process.throwDeprecation=true;process.emitWarning(supplied);result.push(['sync',warnings.length]);
setImmediate(()=>{result.push(warnings);console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--no-warnings", "-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn console_write_error_policy_preserves_identity_and_guards_stream_errors() {
    const SOURCE: &str = r#"
const {Console}=require('console'),EE=require('events');const result=[];
const marker={},overflow=new RangeError('Maximum call stack size exceeded');
for(const ignoreErrors of [undefined,true,false,0,null]){
 const stream=new EE();let writes=[];
 stream.write=function(text,callback){writes.push([text,typeof callback,this.listenerCount('error')]);throw marker};
 const console=new Console({stdout:stream,stderr:stream,ignoreErrors});
 let thrown=false;try{console.log('value')}catch(error){thrown=error===marker}
 result.push([console._ignoreErrors,writes,thrown,stream.listenerCount('error'),Object.getOwnPropertyDescriptor(console,'_ignoreErrors').enumerable]);
}
const stream=new EE();stream.write=()=>{throw overflow};const logger=new Console({stdout:stream});
try{logger.warn('overflow')}catch(error){result.push(['overflow',error===overflow,stream.listenerCount('error')])}
const asyncStream=new EE();asyncStream._writableState={errorEmitted:false};
asyncStream.write=function(text,callback){result.push(['write',text,this.listenerCount('error')]);process.nextTick(()=>{callback(marker);result.push(['before-error',this.listenerCount('error')]);this.emit('error',marker);this._writableState.errorEmitted=true;result.push(['after-error',this.listenerCount('error')])});return true};
new Console({stdout:asyncStream}).log('async');result.push(['after-log',asyncStream.listenerCount('error')]);
setImmediate(()=>{console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[cfg(unix)]
#[test]
fn tcp_peer_reset_preserves_os_errno_syscall_and_error_message() {
    use std::io::Read;
    use std::net::TcpListener;
    use std::os::fd::AsRawFd;
    use std::time::Instant;
    let mut observations = Vec::new();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
                    Err(error) => panic!("peer accept: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket.read_exact(&mut [0u8; 1]).unwrap();
            let linger = libc::linger {
                l_onoff: 1,
                l_linger: 0,
            };
            // The live socket and linger value remain valid for this syscall.
            let status = unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_LINGER,
                    std::ptr::from_ref(&linger).cast(),
                    std::mem::size_of_val(&linger) as libc::socklen_t,
                )
            };
            assert_eq!(status, 0, "{}", std::io::Error::last_os_error());
            drop(socket);
        });
        let source = format!(
            r#"
const socket=require('net').createConnection({{host:'127.0.0.1',port:{port}}});const result=[];
socket.on('connect',()=>{{result.push('connect');socket.write('x')}});
socket.on('error',error=>result.push([error.name,error.message,error.code,error.errno,error.syscall]));
socket.on('close',hadError=>{{result.push(['close',hadError]);console.log(JSON.stringify(result))}});
"#
        );
        let mut command = Command::new(executable);
        command.args(["-e", &source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        observations.push(
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap(),
        );
        peer.join().unwrap();
    }
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(observation.stderr.is_empty());
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn emitter_callable_construction_matches_metadata_defaults_and_numeric_counters() {
    const SOURCE: &str = r#"
const EE=require('events'),result=[];
function view(emitter){return Reflect.ownKeys(emitter).map(key=>[typeof key==='symbol'?String(key):key,typeof key==='symbol'?Symbol.keyFor(key)??null:null,typeof emitter[key]==='object'?'object':emitter[key]??'undefined'])}
const emitter=new EE();result.push(EE.name,EE.length,emitter instanceof EE,view(emitter),Object.getPrototypeOf(emitter._events)===null);
function Derived(){EE.call(this)}Object.setPrototypeOf(Derived.prototype,EE.prototype);
const left=new Derived(),right=new Derived();left.on('x',()=>{});result.push(left instanceof EE,right.listenerCount('x'),left._eventsCount,right._eventsCount);
const original=emitter._events;EE.init.call(emitter);result.push(emitter._events===original,view(emitter));
const inherited=Object.create(emitter);EE.call(inherited);result.push(inherited._events!==emitter._events,inherited._eventsCount);
EE.captureRejections=true;const overridden=new EE({captureRejections:false});const capture=Object.getOwnPropertySymbols(overridden).find(key=>String(key)==='Symbol(kCapture)');result.push(overridden[capture],Object.hasOwn(overridden,'captureRejections'));
const descriptor=Object.getOwnPropertyDescriptor(EE,'captureRejections');result.push(descriptor.enumerable,descriptor.configurable,EE.captureRejections);
EE.captureRejections=false;
for(const captureRejections of [undefined,null,false,0,true,1,'x',{},0n,1n]){
 try{const value=new EE({captureRejections});result.push([captureRejections===true?'true':'other',value[capture]])}catch(error){result.push([error.name,error.code,error.message])}
}
let reads=0;const options={get captureRejections(){reads++;return reads<3}};const counted=new EE(options);result.push(reads,counted[capture]);
for(const value of [undefined,null,1,{},'x']){try{EE.captureRejections=value}catch(error){result.push([error.name,error.code,error.message])}}
const counts=new EE();counts._eventsCount='7';counts.on('x',()=>{});result.push(counts._eventsCount,typeof counts._eventsCount);counts.removeAllListeners('x');result.push(counts._eventsCount);
counts._eventsCount=7n;counts.on('big',()=>{});result.push(String(counts._eventsCount),typeof counts._eventsCount);
const trace=[];counts._eventsCount={valueOf(){trace.push('valueOf');return 3}};counts.on('converted',()=>{});result.push(trace,counts._eventsCount);
const marker={};counts._eventsCount={valueOf(){throw marker}};try{counts.on('throws',()=>{})}catch(error){result.push(error===marker,counts.listenerCount('throws'))}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn emitter_capture_rejections_preserves_thenable_identity_and_tick_order() {
    const SOURCE: &str = r#"
const EE=require('events'),result=[],marker={};
const disabled=new EE();disabled.on('x',()=>({get then(){throw marker}}));result.push(['disabled',disabled.emit('x')]);
const emitter=new EE({captureRejections:true});let gets=0;
const thenable={get then(){gets++;return function(resolve,reject){result.push(['then',this===thenable,resolve===undefined,arguments.length]);reject(marker)}}};
emitter[EE.captureRejectionSymbol]=function(error,type,...args){result.push(['rejection',this===emitter,error===marker,type,args])};
emitter.on('event',()=>thenable);result.push('before');result.push(emitter.emit('event',1,'two'),'after',gets);
const throws=new EE({captureRejections:true});throws.on('error',error=>result.push(['getter-error',error===marker]));throws.on('throws',()=>({get then(){throw marker}}));throws.emit('throws');
const promiseEmitter=new EE({captureRejections:true});const first={},second={};
promiseEmitter.on('event',()=>Promise.reject(first));promiseEmitter.on('error',error=>{result.push(['error',error===first]);return Promise.reject(second)});
process.on('unhandledRejection',error=>result.push(['unhandled',error===second]));promiseEmitter.emit('event');
setImmediate(()=>console.log(JSON.stringify(result)));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn primitive_constructor_names_descriptors_and_prototype_aliases_match_node() {
    const SOURCE: &str = r#"
const result=[];
for(const constructor of [Number,String]){
 const descriptor=Object.getOwnPropertyDescriptor(constructor,'name');
 result.push([constructor.name,constructor.prototype.constructor===constructor,descriptor.value,descriptor.writable,descriptor.enumerable,descriptor.configurable]);
}
result.push((42).constructor.name,'value'.constructor.name,new Number(1).constructor.name,new String('x').constructor.name);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn util_custom_inspection_preserves_depth_receiver_and_hook_order() {
    const SOURCE: &str = r#"
const util=require('util'),inspect=util.inspect,key=Symbol.for('nodejs.util.inspect.custom'),result=[],calls=[];
const object={};Object.defineProperty(object,key,{value:function(depth,options,fn){calls.push([this===object,depth,options.depth,options.customInspect,fn===inspect]);return 'custom'}});
result.push(inspect(object,{depth:-1}),inspect({object},{depth:1}),inspect(object,{depth:null}));
let reads=0;const disabled={};Object.defineProperty(disabled,key,{get(){reads++;throw new Error('getter')}});
for(const customInspect of [false,0,null])result.push(inspect(disabled,{customInspect}));result.push(reads);
const returned={};Object.defineProperty(returned,key,{value(){return {value:1}}});result.push(inspect(returned,{depth:0}));
const self={value:1};Object.defineProperty(self,key,{value(){return this}});result.push(inspect(self));
function customFunction(){}customFunction[key]=()=> 'function hook';result.push(inspect(customFunction));
const error=new Error('unused');error[key]=()=> 'error hook';result.push(inspect(error));
const marker={};const getter={};Object.defineProperty(getter,'constructor',{get(){throw marker}});Object.defineProperty(getter,key,{value(){return 'no constructor getter'}});result.push(inspect(getter));
const prototype=Object.create(null);Object.defineProperty(prototype,'constructor',{value:{prototype}});let prototypeCalls=0;Object.defineProperty(prototype,key,{value(){prototypeCalls++;return 'unexpected'}});inspect(prototype);result.push(prototypeCalls);
const throwing={};Object.defineProperty(throwing,key,{value(){throw marker}});try{inspect(throwing)}catch(error){result.push(error===marker)}
const saved=inspect.custom;inspect.custom=Symbol('replacement');result.push(inspect(object));inspect.custom=saved;
console.log(JSON.stringify({result,calls}));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn dom_event_phase_constants_match_node_descriptors() {
    const SOURCE: &str = r#"
const result=[];
for(const name of ['NONE','CAPTURING_PHASE','AT_TARGET','BUBBLING_PHASE']){const d=Object.getOwnPropertyDescriptor(Event,name);result.push([name,d.value,d.writable,d.enumerable,d.configurable,Object.getOwnPropertyDescriptor(Event.prototype,name)===undefined]);}
const event=new Event('phase');result.push(event.NONE,event.CAPTURING_PHASE,event.AT_TARGET,event.BUBBLING_PHASE);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn emitter_listener_store_tracks_nested_emission_and_copy_on_mutation() {
    const SOURCE: &str = r#"
const EE=require('events'),result=[];
const idle=new EE();idle.setMaxListeners(0);const a=()=>{},b=()=>{},c=()=>{};idle.on('x',a).on('x',b);const list=idle._events.x;idle.on('x',c);result.push(idle._events.x===list,list.length);idle.removeListener('x',b);result.push(idle._events.x===list,list.length,list[0]===a,list[1]===c);
const emitter=new EE();emitter.setMaxListeners(0);const calls=[];let old,key;
function first(){calls.push('first');emitter.removeListener('x',second);const copy=emitter._events.x;emitter.on('x',last);result.push([old[key],copy!==old,copy===emitter._events.x,old.length,copy.length,copy.warned]);}
function second(){calls.push('second')}function third(){calls.push('third')}function last(){calls.push('last')}
emitter.on('x',first).on('x',second).on('x',third);old=emitter._events.x;old.warned=true;key=Object.getOwnPropertySymbols(old).find(s=>s.description==='events.emitting');result.push(typeof key);emitter.emit('x');result.push(calls,old[key],emitter._events.x[key]);
const nested=new EE(),depths=[];let inside=false,nestedList,nestedKey;
nested.on('x',()=>{depths.push(nestedList[nestedKey]);if(!inside){inside=true;nested.emit('x');depths.push(nestedList[nestedKey]);}}).on('x',()=>depths.push(nestedList[nestedKey]));nestedList=nested._events.x;nestedKey=Object.getOwnPropertySymbols(nestedList).find(s=>s.description==='events.emitting');nested.emit('x');result.push(depths,nestedList[nestedKey]);
const throwing=new EE(),marker={};throwing.on('x',a).on('x',()=>{throw marker});const throwingList=throwing._events.x,throwingKey=Object.getOwnPropertySymbols(throwingList).find(s=>s.description==='events.emitting');try{throwing.emit('x')}catch(error){result.push(error===marker,throwingList[throwingKey]);}
const duplicate=new EE();duplicate.on('x',a).on('x',b).on('x',a);duplicate.removeListener('x',a);result.push(duplicate.rawListeners('x').map(fn=>fn===a?'a':'b'));
const once=new EE();once.once('x',a).once('x',a);const wrappers=once.rawListeners('x');once.removeListener('x',a);result.push(once.rawListeners('x')[0]===wrappers[0],once.listenerCount('x'));
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn emitter_removal_preserves_duplicate_notifications_and_numeric_rules() {
    const SOURCE: &str = r#"
const EE=require('events'),result=[],a=()=>{},b=()=>{};
const notify=new EE();notify.on('x',a).on('x',a);const notifications=[];notify.on('removeListener',(type,listener)=>notifications.push([type,listener===a]));notify.removeAllListeners('x');result.push(notifications,notify._eventsCount);
const order=new EE(),calls=[];function one(type){calls.push(['one',type])}function two(type){calls.push(['two',type])}order.on('x',a).on('removeListener',one).on('removeListener',two);order.removeAllListeners();result.push(calls,order._eventsCount,order.eventNames());
const explicit=new EE();explicit.on('undefined',a).on('x',b);explicit.removeAllListeners(undefined);result.push(explicit.eventNames(),explicit._eventsCount);
const numeric=new EE();numeric.on('x',a).on('y',b);numeric._eventsCount=2n;numeric.removeAllListeners('x');result.push(String(numeric._eventsCount),typeof numeric._eventsCount,numeric.listenerCount('x'));try{numeric.removeListener('y',b)}catch(error){result.push(error.name,numeric.listenerCount('y'));}
const changed=new EE();let fired=0;changed.on('newListener',(type)=>{if(type==='newListener')fired++});changed.on('newListener',()=>{});result.push(fired);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn emitter_public_method_metadata_and_once_override_order_match_node() {
    const SOURCE: &str = r#"
const EE=require('events'),result=[],listener=()=>{};
for(const name of ['addListener','on','prependListener','once','prependOnceListener','removeListener','off','removeAllListeners','emit','eventNames','listeners','rawListeners','listenerCount','getMaxListeners','setMaxListeners']){const d=Object.getOwnPropertyDescriptor(EE.prototype,name),fn=d.value;result.push([name,fn.name,fn.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(fn,'prototype')]);}
result.push(EE.prototype.on===EE.prototype.addListener,EE.prototype.off===EE.prototype.removeListener,Object.keys(Reflect.construct(EE.prototype.emit,['x'])));
const overridden=new EE(),calls=[];overridden.on=function(type,wrapper){calls.push([this===overridden,type,wrapper.name,wrapper.length,wrapper.listener===listener]);return 'ignored'};result.push(overridden.once('x',listener)===overridden);
overridden.prependListener=function(type,wrapper){calls.push([this===overridden,type,wrapper.name,wrapper.length,wrapper.listener===listener]);return 'ignored'};result.push(overridden.prependOnceListener('y',listener)===overridden,calls);
const once=new EE(),trace=[];function single(value){trace.push(['called',value])}once.once('x',single);const raw=once.rawListeners('x')[0];once.on('removeListener',(type)=>{if(type==='x'){trace.push('removed');raw('nested')}});once.emit('x','first');raw('again');result.push(trace,raw.listener===single,raw.name,raw.length);
const poison=new EE(),marker={};let fired=0;poison.once('x',()=>fired++);const wrapped=poison.rawListeners('x')[0];poison.removeListener=()=>{throw marker};try{wrapped()}catch(error){result.push(error===marker)}poison.removeListener=EE.prototype.removeListener;wrapped();result.push(fired);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn internal_inspect_retains_canonical_identity_when_public_alias_changes() {
    const SOURCE: &str = r#"
const EE=require('events'),util=require('util'),original=util.inspect,result=[],calls=[];
const emitter=new EE();emitter.setMaxListeners(1);emitter[Symbol.for('nodejs.util.inspect.custom')]=function(depth,options,fn){calls.push([this===emitter,depth,options.depth,fn===original]);return 'owned'};
util.inspect=()=>{throw new Error('public inspector replaced')};
process.emitWarning=warning=>result.push([warning.name,warning.message,warning.emitter===emitter,warning.type,warning.count]);emitter.on('x',()=>{}).on('x',()=>{});
const errorValue={};errorValue[Symbol.for('nodejs.util.inspect.custom')]=function(depth,options,fn){calls.push([depth,fn===original]);return 'payload'};try{new EE().emit('error',errorValue)}catch(error){result.push([error.code,error.message,error.context===errorValue])}
console.log(JSON.stringify({result,calls}));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn typed_array_fill_matches_node_bytes_coercion_and_live_view_ranges() {
    const SOURCE: &str = r#"
const result=[];
for(const Ctor of [Uint8Array,Uint8ClampedArray,Int8Array,Uint16Array,Int16Array,Uint32Array,Int32Array,Float16Array,Float32Array,Float64Array])for(const value of [0,-0,1.5,257,-257,Infinity,NaN]){const whole=new Ctor(5),view=whole.subarray(1,4);const returned=view.fill(value,-2);result.push([Ctor.name,String(value),returned===view,Array.from(new Uint8Array(whole.buffer))]);}
for(const Ctor of [BigInt64Array,BigUint64Array])for(const value of [0n,-1n,(2n**70n)+17n,-(2n**70n)-17n]){const whole=new Ctor(4),view=whole.subarray(1,3);view.fill(value);result.push([Ctor.name,String(value),Array.from(new Uint8Array(whole.buffer))]);}
const trace=[],buffer=new ArrayBuffer(8,{maxByteLength:16}),tracking=new Uint8Array(buffer);tracking.fill({valueOf(){trace.push('value');return 7}},{valueOf(){trace.push('start');buffer.resize(3);return 1}},{valueOf(){trace.push('end');buffer.resize(6);return 5}});result.push(trace,Array.from(tracking));
const fixedBuffer=new ArrayBuffer(8,{maxByteLength:16}),fixed=new Uint8Array(fixedBuffer,2,4);try{fixed.fill(9,{valueOf(){fixedBuffer.resize(3);return 0}})}catch(error){result.push(error.name,fixed.length)}
const shared=new Uint16Array(new SharedArrayBuffer(8));shared.fill(0x4444,1,3);result.push(Array.from(shared));
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn util_text_codec_aliases_capture_initialized_global_constructors() {
    const SOURCE: &str = r#"
const util=require('util'),result=[],encoder=util.TextEncoder,decoder=util.TextDecoder;
result.push(typeof encoder,typeof decoder,encoder===globalThis.TextEncoder,decoder===globalThis.TextDecoder,encoder.name,decoder.name);
globalThis.TextEncoder=function replacedEncoder(){};globalThis.TextDecoder=function replacedDecoder(){};
result.push(require('util')===util,util.TextEncoder===encoder,util.TextDecoder===decoder);
const bytes=new encoder().encode('A€𤭢');result.push(Array.from(bytes),new decoder().decode(bytes));
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[test]
fn trace_categories_retain_enabled_objects_and_capture_array_policy() {
    const SOURCE: &str = r#"
const api=require('trace_events'),util=require('util'),result=[];
for(const value of [undefined,null,1,'x',true,[],()=>{}])try{api.createTracing(value)}catch(e){result.push([e.name,e.code])}
for(const categories of [undefined,null,'x',[1],[]])try{api.createTracing({categories})}catch(e){result.push([e.name,e.code])}
const categories=['z','a','a'],tracing=api.createTracing({categories});
result.push(tracing.constructor.name,tracing.enabled,tracing.categories,Reflect.ownKeys(tracing).length,api.getEnabledCategories());
const direct=new tracing.constructor(['direct']);result.push(direct.categories,direct.enabled);direct.enable();result.push(api.getEnabledCategories());direct.disable();
const proto=Object.getPrototypeOf(tracing);for(const name of ['enabled','categories']){const d=Object.getOwnPropertyDescriptor(proto,name);try{d.get.call({})}catch(e){result.push([e.name,e.message])}}

tracing.enable();tracing.enable();result.push(tracing.enabled,api.getEnabledCategories());categories[0]='mutated';result.push(tracing.categories,api.getEnabledCategories(),util.inspect(tracing));
tracing.disable();tracing.disable();result.push(tracing.enabled,api.getEnabledCategories());tracing.enable();result.push(api.getEnabledCategories());tracing.disable();
let reads=0;const dynamic=api.createTracing({get categories(){reads++;return ['q']}});result.push(reads,dynamic.categories);
let unreachable=api.createTracing({categories:['retained']});unreachable.enable();unreachable=undefined;gc();result.push(api.getEnabledCategories());
const warnings=[];process.emitWarning=message=>warnings.push(message);const objects=[];for(let i=0;i<11;i++){const t=api.createTracing({categories:['x']});t.enable();objects.push(t)}result.push(warnings);for(const t of objects)t.disable();
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new(executable);
        command
            .current_dir(directory.path())
            .args(["--expose-gc", "--no-warnings", "-e", SOURCE]);
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
        assert!(!observation.stdout.is_empty());
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn console_counters_and_timer_coercion_order_match_node() {
    const SOURCE: &str = r#"
const {Console}=require('console'),result=[],sink={write(){},on(){},removeListener(){}};
const c=new Console(sink,sink);c.log=(...args)=>result.push(['log',...args]);process.emitWarning=message=>result.push(['warning',message]);
c.count('x');c.count('x');c.countReset('x');c.countReset('x');c.count('x');c.count({toString(){return 'object'}});c.countReset({toString(){return 'object'}});
try{c.count(Symbol('x'))}catch(e){result.push(['error',e.name])}
process.hrtime=previous=>previous===undefined?[1,0]:[0,123000];
c.time('a');c.time('a');c.timeLog('a','extra');c.timeEnd('a');c.timeLog('a');c.timeEnd('a');
const trace=[],label={toString(){trace.push('label');return 'b'}};c.time(label);c.timeEnd(label);result.push(trace);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--no-warnings", "-e", SOURCE]);
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
fn tracing_writes_console_events_to_expanded_file_pattern() {
    const SOURCE: &str = "console.count('x');console.count('x');console.countReset('x');console.time('a');setImmediate(()=>{console.timeLog('a');setImmediate(()=>console.timeEnd('a'))})";
    let projections = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args([
            "--trace-event-categories",
            "node.console",
            "--trace-event-file-pattern",
            "${pid}-${rotation}.trace.json",
            "-e",
            SOURCE,
        ]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(observation.stderr.is_empty());
        let paths: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(paths.len(), 1);
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&paths[0]).unwrap()).unwrap();
        let events = document["traceEvents"].as_array().unwrap();
        let pid = events[0]["pid"].as_u64().unwrap();
        assert_eq!(
            paths[0].file_name().unwrap().to_string_lossy(),
            format!("{pid}-1.trace.json")
        );
        events
            .iter()
            .filter(|event| event["cat"] != "__metadata")
            .map(|event| {
                assert_eq!(event["pid"].as_u64(), Some(pid));
                serde_json::json!([event["cat"], event["name"], event["ph"], event["args"]])
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(projections[0], projections[1]);
}

#[test]
fn before_exit_restarts_referenced_work_and_precedes_exit() {
    const SOURCE: &str = r#"
const result=[];let visits=0;process.on('beforeExit',code=>{result.push(['beforeExit',code]);if(visits++===0)setImmediate(()=>result.push(['immediate']));});process.on('exit',code=>{result.push(['exit',code]);console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn child_launches_inherit_guest_cwd_and_resolve_relative_overrides() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("nested/other")).unwrap();
    std::fs::write(
        directory.path().join("child.cjs"),
        "console.log(require('path').basename(process.cwd()))",
    )
    .unwrap();
    const SOURCE: &str = r#"
const cp=require('child_process'),path=require('path'),original=process.cwd(),child=path.join(original,'child.cjs'),result=[];process.chdir('nested');
for(const [tag,options] of [['default',{}],['relative',{cwd:'other'}],['absolute',{cwd:original}]]){const r=cp.spawnSync(process.execPath,['-e',"console.log(require('path').basename(process.cwd()))"],options);result.push([tag,r.status,r.stdout.toString().trim(),!!r.error]);}
let pending=3;for(const [tag,options] of [['async-default',{}],['async-relative',{cwd:'other'}],['fork-default',{silent:true,execArgv:[]}]]){const p=tag==='fork-default'?cp.fork(child,[],options):cp.spawn(process.execPath,['-e',"console.log(require('path').basename(process.cwd()))"],options);let text='';p.stdout.on('data',chunk=>text+=chunk);p.on('close',code=>{result.push([tag,code,text.trim()]);if(--pending===0)console.log(JSON.stringify(result.sort((a,b)=>a[0].localeCompare(b[0]))));});}
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn tracing_intrinsics_validate_in_order_and_serialize_with_original_json() {
    const SOURCE: &str = r#"
const {internalBinding}=require('internal/test/binding'),{trace,isTraceCategoryEnabled}=internalBinding('trace_events'),original=JSON.stringify,result=[],calls=[];
for(const args of [[0,1,'x'],['bad','disabled',null],['bad','custom','x'],[98,'custom',1],[98,'custom','x','id'],[98,'custom','']])try{result.push(trace(...args))}catch(e){result.push([e.name,e.message])}
try{isTraceCategoryEnabled({toString(){throw 'coercion'}})}catch(e){result.push([e.name,e.message])}
JSON.stringify=()=>{throw 'public JSON'};
const marker={};try{trace(98,'custom','throw',1,{toJSON(){throw marker}})}catch(e){result.push(e===marker)}
result.push(trace(98,'disabled','ignored',null,{toJSON(){throw 'disabled data'}}));
const data={toJSON(key){calls.push(['toJSON',key]);return {get value(){calls.push('value');return 7},list:[null,true,'text']}}};
result.push(trace(98,'custom','payload',-1,data),trace(101,'custom','payload',-1),isTraceCategoryEnabled('custom'),isTraceCategoryEnabled('disabled'));
console.log(original({result,calls}));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args([
            "--no-warnings",
            "--expose-internals",
            "--trace-event-categories=custom",
            "-e",
            SOURCE,
        ]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(observation.stderr.is_empty());
        let file: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("node_trace.1.log")).unwrap(),
        )
        .unwrap();
        let events = file["traceEvents"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["cat"] != "__metadata")
            .map(|e| serde_json::json!([e["ph"], e["cat"], e["name"], e["id"], e["args"]]))
            .collect::<Vec<_>>();
        (observation.stdout, events)
    });
    assert_eq!(observations[0], observations[1]);
}

#[test]
fn trace_category_buffers_follow_activation_without_retaining_dead_views() {
    const SOURCE: &str = r#"
const {internalBinding}=require('internal/test/binding'),binding=internalBinding('trace_events'),api=require('trace_events'),result=[];
const a=binding.getCategoryEnabledBuffer('custom'),b=binding.getCategoryEnabledBuffer('custom'),other=binding.getCategoryEnabledBuffer('other');result.push(a.constructor.name,a.length,a.byteLength,a.byteOffset,a.buffer.byteLength,a===b,a[0],b[0],other[0]);
const first=api.createTracing({categories:['custom']}),second=api.createTracing({categories:['custom']});first.enable();result.push(a[0],b[0],other[0]);second.enable();first.disable();result.push(a[0],b[0]);second.disable();result.push(a[0],b[0]);
for(let i=0;i<20;i++)binding.getCategoryEnabledBuffer('unreachable');gc();first.enable();result.push(a[0],b[0]);first.disable();
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args([
            "--expose-gc",
            "--expose-internals",
            "--no-warnings",
            "-e",
            SOURCE,
        ]);
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
fn environment_tracing_records_real_spans_and_matching_lifetime_ids() {
    const SOURCE: &str = "setImmediate(()=>{});setTimeout(()=>{},1)";
    let projections = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args([
            "--trace-event-categories=node.environment",
            "-e",
            SOURCE,
        ]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(observation.stderr.is_empty());
        let file: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("node_trace.1.log")).unwrap(),
        )
        .unwrap();
        let events = file["traceEvents"].as_array().unwrap();
        let lifetime: Vec<_> = events
            .iter()
            .filter(|e| e["name"] == "Environment")
            .collect();
        assert_eq!(lifetime.len(), 2);
        assert_eq!(lifetime[0]["id"], lifetime[1]["id"]);
        assert_eq!(lifetime[0]["ph"], "b");
        assert_eq!(lifetime[1]["ph"], "e");
        assert_eq!(
            lifetime[0]["args"]["args"]["exec_args"],
            serde_json::json!(["--trace-event-categories=node.environment", "-e", SOURCE])
        );
        let pid = lifetime[0]["pid"].as_u64().unwrap();
        let mut projected = std::collections::BTreeSet::new();
        for event in events.iter().filter(|e| e["cat"] != "__metadata") {
            assert_eq!(event["pid"].as_u64(), Some(pid));
            assert!(event["ts"].as_u64().is_some());
            assert!(event["tts"].as_u64().is_some());
            assert!(event["dur"].as_u64().is_some());
            assert!(event["tdur"].as_u64().is_some());
            projected.insert((
                event["name"].as_str().unwrap().to_owned(),
                event["ph"].as_str().unwrap().to_owned(),
            ));
        }
        projected
    });
    assert_eq!(projections[0], projections[1]);
}

#[test]
fn before_exit_routes_original_exceptions_before_restarting_work() {
    const SOURCE: &str = r#"
const marker={get stack(){throw 'handled stack read'}},result=[];let calls=0;
process.on('beforeExit',()=>{result.push('before');if(calls++===0)throw marker});
process.on('uncaughtException',error=>{result.push(error===marker);setImmediate(()=>result.push('immediate'))});
process.on('exit',()=>console.log(JSON.stringify(result)));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn fatal_before_exit_errors_retain_diagnostics_and_final_exit_status() {
    for status in [0, 7] {
        let source = format!(
            "process.on('exit',()=>{{process.exitCode={status}}});process.on('beforeExit',()=>{{throw new Error('before-exit-marker')}})"
        );
        for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
            let mut command = Command::new(executable);
            command.args(["-e", &source]);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            let observation =
                quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                    .unwrap();
            assert!(!observation.timed_out);
            assert_eq!(
                observation.exit_code,
                Some(status),
                "{}",
                String::from_utf8_lossy(&observation.stderr)
            );
            assert!(observation.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&observation.stderr).contains("Error: before-exit-marker")
            );
        }
    }
}

#[test]
fn borrowed_emit_and_domain_errors_preserve_original_values() {
    const SOURCE: &str = r#"
const assert=require('assert'),{EventEmitter}=require('events');
const marker=new Error('borrowed');const bare=Object.create(null);
try{EventEmitter.prototype.emit.call(bare,'error',marker);throw 'missing error'}catch(error){assert.strictEqual(error,marker)}
assert.strictEqual(Object.hasOwn(bare,'_events'),false);
assert.strictEqual(EventEmitter.prototype.emit.call({},'unused'),false);
const domain=require('domain'),result=[];
for(const value of [undefined,null,false,0,'',42,'fortytwo',true,marker]){
 const emitter=new EventEmitter(),owner=domain.create();owner.add(emitter);
 owner.on('error',error=>{
  result.push(value?error===value:error instanceof Error);
  if(!value)result.push(error.code,error.message);
  if(error&&typeof error==='object')result.push(error.domain===owner,error.domainEmitter===emitter,error.domainThrown===false,Object.getOwnPropertyDescriptor(error,'domain').enumerable===false);
 });emitter.emit('error',value);
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn static_event_listeners_project_records_and_delegate_public_hooks() {
    const SOURCE: &str = r#"
const assert=require('assert'),{EventEmitter,getEventListeners}=require('events');
function a(){}function b(){}
for(const target of [new EventEmitter(),new EventTarget()]){
 const add=target.on||target.addEventListener,remove=target.off||target.removeEventListener;
 add.call(target,'x',a);add.call(target,'x',b);
 const snapshot=getEventListeners(target,'x');assert.deepStrictEqual(snapshot,[a,b]);
 snapshot.length=0;assert.deepStrictEqual(getEventListeners(target,'x'),[a,b]);
 remove.call(target,'x',a);assert.deepStrictEqual(getEventListeners(target,'x'),[b]);
 assert.deepStrictEqual(getEventListeners(target,'absent'),[]);
}
const calls=[],original={get listeners(){calls.push('get');return function(type){calls.push(this===original,type);return 42}}};
assert.strictEqual(getEventListeners(original,'custom'),42);
console.log(JSON.stringify([getEventListeners.name,getEventListeners.length,calls]));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn abort_listener_disposal_and_stopped_propagation_match_node() {
    const SOURCE: &str = r#"
const assert=require('assert'),{addAbortListener,getEventListeners}=require('events');
const result=[],controller=new AbortController(),signal=controller.signal;
signal.addEventListener('abort',e=>{result.push('stop');e.stopImmediatePropagation()});
signal.addEventListener('abort',()=>result.push('ordinary'));
const disposable=addAbortListener(signal,e=>result.push('resist',e.target===signal));
assert.strictEqual(Object.getPrototypeOf(disposable),null);
assert.strictEqual(disposable[Symbol.dispose].length,0);
const removed=addAbortListener(signal,()=>result.push('removed'));removed[Symbol.dispose]();removed[Symbol.dispose]();
controller.abort();disposable[Symbol.dispose]();
const already=addAbortListener(AbortSignal.abort(),function(){'use strict';result.push('queued',this===undefined,arguments.length)});
already[Symbol.dispose]();result.push('sync');
const activeSignal=AbortSignal.abort(),EventConstructor=Event;let delivered;
const active=addAbortListener(activeSignal,function(event){'use strict';delivered=event;assert(event instanceof EventConstructor);assert.strictEqual(event.type,'abort');assert.strictEqual(event.target,activeSignal);assert.strictEqual(event.currentTarget,activeSignal);assert.strictEqual(event.isTrusted,true);result.push('active',this===undefined,arguments.length)});
globalThis.Event=function(){throw Error('public Event constructor was used')};
for(const bad of [undefined,null,{},1])assert.throws(()=>addAbortListener(bad,()=>{}),{code:'ERR_INVALID_ARG_TYPE'});
for(const bad of [undefined,null,{}])assert.throws(()=>addAbortListener(new AbortController().signal,bad),{code:'ERR_INVALID_ARG_TYPE'});
let options,removedCalls=0;const foreign={aborted:false,addEventListener(type,callback,value){options=value},removeEventListener(){removedCalls++}};
const foreignDisposable=addAbortListener(foreign,()=>{});foreignDisposable[Symbol.dispose]();foreignDisposable[Symbol.dispose]();
assert.strictEqual(Object.getPrototypeOf(options),null);assert(Object.isFrozen(options));assert.strictEqual(options.once,true);
result.push(removedCalls,Object.getOwnPropertySymbols(options).length);
queueMicrotask(()=>{globalThis.Event=EventConstructor;assert.strictEqual(delivered.currentTarget,null);active[Symbol.dispose]();console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn abort_onabort_uses_the_ordered_handler_wrapper() {
    const SOURCE: &str = r#"
const assert=require('assert'),{getEventListeners,listenerCount}=require('events'),result=[];
const c=new AbortController(),s=c.signal;
function first(){result.push('first')}function original(){result.push('old')}function replacement(){result.push('handler')}
s.addEventListener('abort',first);s.onabort=original;
const wrapper=getEventListeners(s,'abort')[1];
assert.notStrictEqual(wrapper,original);assert.strictEqual(wrapper.name,'eventHandler');assert.strictEqual(wrapper.handler,original);
s.addEventListener('abort',()=>result.push('last'));s.onabort=replacement;
assert.strictEqual(s.onabort,replacement);assert.strictEqual(getEventListeners(s,'abort')[1],wrapper);
assert.strictEqual(wrapper.handler,replacement);c.abort();
const d=new AbortController();d.signal.onabort={handleEvent(){result.push('must not call')}};
assert.strictEqual(typeof d.signal.onabort,'object');d.abort();
const inactive=new AbortController().signal;
for(const value of [null,original,null,{},replacement,replacement,null]){
 inactive.onabort=value;result.push(listenerCount(inactive,'abort'),getEventListeners(inactive,'abort').length);
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn error_monitors_use_public_emit_and_capture_listener_mutations() {
    const SOURCE: &str = r#"
const assert=require('assert'),{EventEmitter,errorMonitor}=require('events'),result=[];
const emitter=new EventEmitter(),original=emitter.emit,marker=new Error('original');
emitter.emit=function(type,...args){result.push(typeof type);return original.call(this,type,...args)};
emitter.on(errorMonitor,error=>{assert.strictEqual(error,marker);result.push('monitor');emitter.on('error',error=>{assert.strictEqual(error,marker);result.push('handler')})});
assert.strictEqual(emitter.emit('error',marker),true);
let reads=0;const borrowed=Object.create(EventEmitter.prototype),records={error:error=>assert.strictEqual(error,marker)};
Object.defineProperty(borrowed,'_events',{get(){reads++;return records}});
assert.strictEqual(borrowed.emit('error',marker),true);assert.strictEqual(reads,1);
const replaced=new EventEmitter();replaced.on(errorMonitor,()=>{replaced._events={error:()=>{throw 'wrong replacement handler'}}});
assert.throws(()=>replaced.emit('error',marker),error=>error===marker);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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

#[cfg(target_os = "linux")]
#[test]
fn children_inherit_directory_identity_after_rename_and_unlink() {
    const SOURCE: &str = r#"
const assert=require('assert'),fs=require('fs'),path=require('path'),cp=require('child_process');
const parent=process.cwd(),preload=path.join(parent,'preload.cjs'),old=path.join(parent,'old'),renamed=path.join(parent,'renamed');
fs.mkdirSync(old);process.chdir(old);fs.renameSync(old,renamed);
assert.strictEqual(process.cwd(),renamed);
const read='console.log(process.cwd())';
let child=cp.spawnSync(process.execPath,['-e',read]);
assert.strictEqual(child.status,0);assert.strictEqual(child.stdout.toString().trim(),renamed);assert.strictEqual(child.stderr.toString(),'');
fs.rmdirSync(renamed);
assert.strictEqual(process.cwd(),renamed);
process.chdir(parent);const uncached=path.join(parent,'uncached');fs.mkdirSync(uncached);process.chdir(uncached);fs.rmdirSync(uncached);
assert.throws(()=>process.cwd(),{code:'ENOENT',syscall:'uv_cwd',errno:-2});
child=cp.spawnSync(process.execPath,['-e','0']);assert.strictEqual(child.status,0);assert.strictEqual(child.stderr.length,0);
child=cp.spawnSync(process.execPath,['-r',preload,'-e','console.log(globalThis.fromPreload)']);
assert.strictEqual(child.status,0);assert.strictEqual(child.stdout.toString(),'42\n');assert.strictEqual(child.stderr.toString(),'');
child=cp.spawnSync(process.execPath,['-e',read],{cwd:'..'});
assert.strictEqual(child.status,0);assert.strictEqual(child.stdout.toString().trim(),parent);
const asyncChild=cp.spawn(process.execPath,['-e','0']);
asyncChild.on('error',error=>{throw error});asyncChild.on('exit',(status,signal)=>{assert.strictEqual(status,0);assert.strictEqual(signal,null);console.log('directory-identity')});
"#;
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("preload.cjs"),
            "globalThis.fromPreload=42",
        )
        .unwrap();
        let mut command = Command::new(executable);
        command.current_dir(directory.path()).args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(observation.stderr.is_empty());
        assert_eq!(observation.stdout, b"directory-identity\n");
    }
}

#[test]
fn coded_argument_errors_preserve_prototype_shape_and_constant_code_rendering() {
    const SOURCE: &str = r#"
const {getEventListeners}=require('events'),assert=require('assert');
let first,second;try{getEventListeners('INVALID_EMITTER')}catch(error){first=error}try{getEventListeners(42)}catch(error){second=error}
assert(first instanceof TypeError);assert.strictEqual(first.constructor,TypeError);
const prototype=Object.getPrototypeOf(first),symbol=Object.getOwnPropertySymbols(prototype)[0];
const result=[first.name,first.code,first.message,String(first),Reflect.ownKeys(first),
 prototype===Object.getPrototypeOf(second),prototype===TypeError.prototype,
 Reflect.ownKeys(prototype).map(key=>typeof key==='symbol'?key.description:key),
 prototype[symbol],first.toString.name,first.toString.length,Object.hasOwn(first.toString,'prototype')];
for(const name of ['code','message']){const d=Object.getOwnPropertyDescriptor(first,name);result.push([d.writable,d.enumerable,d.configurable])}
first.code='changed';result.push(String(first));
result.push(first.toString.call({name:'Custom',message:'marker',get code(){throw 'code must be captured'}}));
const marker={};assert.throws(()=>first.toString.call({get name(){throw marker}}),error=>error===marker);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn posix_resolve_validates_only_consumed_paths_and_queries_cwd_lazily() {
    const SOURCE: &str = r#"
const assert=require('assert'),path=require('path').posix,marker={},result=[];let calls=0;
process.cwd=()=>{calls++;throw marker};
assert.strictEqual(path.resolve('/absolute','leaf'),'/absolute/leaf');
assert.strictEqual(path.resolve({get toString(){throw 'unused path'}},'/absolute','leaf'),'/absolute/leaf');
assert.throws(()=>path.resolve('/absolute',42),{code:'ERR_INVALID_ARG_TYPE'});
assert.strictEqual(calls,0);
for(const args of [[],['.'],[''],['relative']])assert.throws(()=>path.resolve(...args),error=>error===marker);
result.push(calls);process.cwd=()=>{calls++;return '/parent/base'};
result.push(path.resolve('relative'),path.resolve('/root','..','leaf'),calls);
for(const args of [[],['.'],['']]) {let reads=0;process.cwd=()=>++reads===1?'relative':'/second';result.push(path.resolve(...args),reads)}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn async_resource_hooks_preserve_scope_identity_receiver_and_destroy_order() {
    const SOURCE: &str = r#"
const assert=require('assert'),hooks=require('async_hooks'),result=[];
const outerId=hooks.executionAsyncId(),outerTrigger=hooks.triggerAsyncId(),outerResource=hooks.executionAsyncResource();let id,resource,hook;
const callbacks={init(value,type,trigger,current){if(type==='probe'){id=value;resource=current;result.push('init',this===hook,trigger,hooks.executionAsyncId()===outerId,hooks.executionAsyncResource()===outerResource)}},before(value){if(value===id)result.push('before',hooks.executionAsyncId()===id,hooks.triggerAsyncId(),hooks.executionAsyncResource()===resource)},after(value){if(value===id)result.push('after',hooks.executionAsyncId()===id)},destroy(value){if(value===id)result.push('destroy')}};
hook=hooks.createHook(callbacks);callbacks.init=()=>{throw 'callback mutation must not replace copied slot'};hook.enable();
const value=new hooks.AsyncResource('probe',{triggerAsyncId:7,requireManualDestroy:true});
assert.strictEqual(value.triggerAsyncId(),7);assert.strictEqual(value.asyncId(),id);
assert.strictEqual(hooks.executionAsyncId(),outerId);assert.strictEqual(hooks.triggerAsyncId(),outerTrigger);
const marker={};assert.throws(()=>value.runInAsyncScope(function(){'use strict';result.push('call',this===undefined);throw marker}),error=>error===marker);
assert.strictEqual(hooks.executionAsyncId(),outerId);assert.strictEqual(hooks.triggerAsyncId(),outerTrigger);
assert.strictEqual(hooks.executionAsyncResource(),outerResource);assert.strictEqual(value.emitDestroy(),value);
Promise.resolve().then(()=>result.push('promise'));
setImmediate(()=>{result.push('immediate');hook.disable();console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn async_hooks_keep_active_snapshot_during_nested_subscription_changes() {
    const SOURCE: &str = r#"
const assert=require('assert'),{createHook,AsyncResource}=require('async_hooks'),result=[];let first,second;
second=createHook({init(id,type){if(['outer','nested','later'].includes(type))result.push('second',type)}});
first=createHook({init(id,type){if(!['outer','nested','later'].includes(type))return;result.push('first',type);if(type==='outer'){first.disable();second.enable();new AsyncResource('nested')}}});
first.enable();new AsyncResource('outer');new AsyncResource('later');second.disable();
const reads=[],marker={};assert.throws(()=>createHook({get init(){reads.push('init');return 1},get before(){reads.push('before');throw marker}}),error=>error===marker);
const sample=createHook({}),prototype=Object.getPrototypeOf(sample),symbol=Object.getOwnPropertySymbols(sample).find(key=>key.description==='init');let copied=0;
Object.defineProperty(prototype,symbol,{configurable:true,set(value){copied++}});
assert.throws(()=>createHook({promiseResolve(){},trackPromises:false}),{code:'ERR_INVALID_ARG_VALUE'});delete prototype[symbol];
console.log(JSON.stringify([result,reads,copied]));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn emitter_async_resource_preserves_brands_hooks_and_scoped_dispatch() {
    const SOURCE: &str = r#"
const assert=require('assert'),events=require('events'),hooks=require('async_hooks'),result=[];
const E=events.EventEmitterAsyncResource;assert.strictEqual(E,events.EventEmitterAsyncResource);
const descriptor=Object.getOwnPropertyDescriptor(events,'EventEmitterAsyncResource');
result.push(E.name,E.length,descriptor.enumerable,descriptor.configurable,descriptor.get.name);
let id,resource,owner;const outer=hooks.executionAsyncId();const hook=hooks.createHook({init(value,type,trigger,current){if(type==='Foo'){id=value;resource=current;result.push('init',trigger===outer);assert.throws(()=>current.eventEmitter,/Cannot read private member/)}},before(value){if(value===id)result.push('before')},after(value){if(value===id)result.push('after')},destroy(value){if(value===id)result.push('destroy')}}).enable();
class Foo extends E {}owner=new Foo();assert.strictEqual(owner.asyncId,id);assert.strictEqual(owner.triggerAsyncId,outer);assert.strictEqual(owner.asyncResource,resource);assert.strictEqual(resource.eventEmitter,owner);
owner.on('value',function(a,b){result.push('listener',this===owner,hooks.executionAsyncId()===id,hooks.executionAsyncResource()===resource,a,b)});
assert.strictEqual(owner.emit('value',1,2),true);assert.strictEqual(hooks.executionAsyncId(),outer);assert.strictEqual(owner.emitDestroy(),undefined);
assert.throws(()=>E.prototype.emit('value'),/Cannot read private member/);assert.throws(()=>E.prototype.emitDestroy(),/Cannot read private member/);
for(const name of ['asyncId','triggerAsyncId','asyncResource'])assert.throws(()=>Reflect.get(E.prototype,name,{}),/Cannot read private member/);
assert.throws(()=>new E(),{code:'ERR_INVALID_ARG_TYPE'});assert.throws(()=>new E({name:42}),{code:'ERR_INVALID_ARG_TYPE'});
let reads=0;const named=new E({get name(){reads++;return 'Named'},triggerAsyncId:7});assert.strictEqual(named.triggerAsyncId,7);result.push(reads,named.asyncResource.eventEmitter===named);
setImmediate(()=>{hook.disable();console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn child_cwd_accepts_file_urls_and_bytes_and_rejects_invalid_paths_before_launch() {
    const SOURCE: &str = r#"
const assert=require('assert'),cp=require('child_process'),url=require('url'),result=[];
for(const cwd of [process.cwd(),url.pathToFileURL(process.cwd()),Buffer.from(process.cwd())]){
 const child=cp.spawnSync(process.execPath,['-e','console.log(process.cwd())'],{cwd});
 assert.strictEqual(child.status,0);assert.strictEqual(child.stdout.toString().trim(),process.cwd());assert.strictEqual(child.stderr.length,0);result.push('cwd');
}
const bytes=new Uint8Array(Buffer.from(process.cwd())),byteChild=cp.spawnSync(process.execPath,['-e','0'],{cwd:bytes});assert.strictEqual(byteChild.status,null);assert.strictEqual(byteChild.error.code,'ENOENT');
const overridden=Buffer.from('unused');overridden.toString=()=>process.cwd();const overriddenChild=cp.spawnSync(process.execPath,['-e','0'],{cwd:overridden});assert.strictEqual(overriddenChild.status,0);result.push('coerced');
for(const spawn of [cp.spawn,cp.spawnSync]){
 for(const cwd of [1,true,{},[],()=>{}])assert.throws(()=>spawn(process.execPath,['-e','0'],{cwd}),{code:'ERR_INVALID_ARG_TYPE'});
 assert.throws(()=>spawn(process.execPath,['-e','0'],{cwd:new URL('http://example.com/')}),{code:'ERR_INVALID_URL_SCHEME'});
 assert.throws(()=>spawn(process.execPath,['-e','0'],{cwd:new URL('file://host/tmp')}),{code:'ERR_INVALID_FILE_URL_HOST'});
 for(const cwd of ['bad\0path',Buffer.from([0])])assert.throws(()=>spawn(process.execPath,['-e','0'],{cwd}),{code:'ERR_INVALID_ARG_VALUE'});
 result.push('validated');
}
const child=cp.spawn(process.execPath,['-e','0'],{cwd:url.pathToFileURL(process.cwd())});child.on('error',error=>{throw error});child.on('exit',code=>{assert.strictEqual(code,0);console.log(JSON.stringify(result))});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn tls_context_material_uses_real_crl_and_pkcs12_parsers() {
    use openssl::{
        asn1::Asn1Time,
        hash::MessageDigest,
        pkcs12::Pkcs12,
        pkey::PKey,
        rsa::Rsa,
        x509::{X509, X509NameBuilder},
    };
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "quench-parser-oracle")
        .unwrap();
    let name = name.build();
    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_pubkey(&key).unwrap();
    certificate
        .set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    certificate
        .set_not_after(&Asn1Time::days_from_now(1).unwrap())
        .unwrap();
    certificate.sign(&key, MessageDigest::sha256()).unwrap();
    let certificate = certificate.build();
    let mut archive = Pkcs12::builder();
    archive.pkey(&key).cert(&certificate);
    let archive = archive
        .build2("distinct-generated-password")
        .unwrap()
        .to_der()
        .unwrap();
    let mut no_key = Pkcs12::builder();
    no_key.cert(&certificate);
    let no_key = no_key.build2("distinct-generated-password").unwrap().to_der().unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let crl = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node/test/fixtures/keys/ca2-crl.pem");
    const SOURCE: &str = r#"
const assert=require('assert'),tls=require('tls'),fs=require('fs'),result=[];
const pfx=Buffer.from(ARCHIVE_HEX,'hex'),noKey=Buffer.from(NO_KEY_HEX,'hex'),crl=fs.readFileSync(CRL_PATH),passphrase='distinct-generated-password';
assert.strictEqual(tls.createSecureContext.length,1);
for(const options of [undefined,null,false,0,'',{}, {crl}, {crl:crl.toString()}, {crl:[crl,crl]}, {pfx,passphrase}, {pfx:new Uint8Array(pfx),passphrase}, {pfx:[{buf:pfx,passphrase}],passphrase:'wrong'}]) { assert(tls.createSecureContext(options).context);result.push('ok'); }
for(const options of [1,'x',[],{crl:''},{crl:'arbitrary malformed revocation data'},{crl:Buffer.from('invalid pem')},{crl:3},{pfx:3},{pfx:''},{pfx:Buffer.alloc(0)},{pfx:Buffer.from('different malformed archive')},{pfx},{pfx,passphrase:'wrong'},{pfx:noKey,passphrase}]){
 try{tls.createSecureContext(options);assert.fail('expected parser rejection')}catch(error){result.push([error.name,error.code,error.message,'opensslErrorStack' in error]);}
}
console.log(JSON.stringify(result));
"#;
    let source = SOURCE
        .replace(
            "ARCHIVE_HEX",
            &serde_json::to_string(&hex(&archive)).unwrap(),
        )
        .replace("NO_KEY_HEX", &serde_json::to_string(&hex(&no_key)).unwrap())
        .replace("CRL_PATH", &serde_json::to_string(&crl).unwrap());
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", &source]);
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
fn synchronous_child_outputs_preserve_buffer_identity_encoding_and_result_shape() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert'),result=[];
const source='process.stdout.write(Buffer.from("00ff4162","hex"));process.stderr.write("err")';
for(const encoding of [undefined,'buffer','utf8','hex','base64','latin1']){
 const child=cp.spawnSync(process.execPath,['-e',source],{encoding});
 assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);assert.strictEqual(Object.hasOwn(child,'error'),false);
 assert.strictEqual(child.output[0],null);assert.strictEqual(child.output[1],child.stdout);assert.strictEqual(child.output[2],child.stderr);
 result.push([encoding,Object.keys(child),Buffer.isBuffer(child.stdout),Buffer.isBuffer(child.stderr),child.stdout.toString(),child.stderr.toString()]);
}
const missing=cp.spawnSync('quench-oracle-command-does-not-exist');assert.strictEqual(missing.pid,0);assert.strictEqual(missing.output,null);assert.strictEqual(missing.stdout,undefined);assert.strictEqual(missing.stderr,undefined);result.push([Object.keys(missing),missing.error.code,missing.error.message]);
const original=Buffer.from,marker={};Buffer.from=()=>{throw marker};try{const child=cp.spawnSync(process.execPath,['-e','process.stdout.write("safe")']);assert(Buffer.isBuffer(child.stdout));assert.strictEqual(child.stdout.toString(),'safe');}finally{Buffer.from=original;}
const toString=Buffer.prototype.toString,trace=[];Buffer.prototype.toString=function(encoding){trace.push([encoding,this.length]);return Reflect.apply(toString,this,[encoding]);};
try{const child=cp.spawnSync(process.execPath,['-e','process.stdout.write("a");process.stderr.write("b")'],{encoding:'hex'});assert.strictEqual(child.stdout,'61');assert.strictEqual(child.stderr,'62');}finally{Buffer.prototype.toString=toString;}
result.push(trace);assert.strictEqual(cp.execFileSync(process.execPath,['-e','process.stdout.write("a")'],{encoding:'hex'}),'61');console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn process_output_streams_preserve_raw_view_bytes_and_reject_invalid_chunks() {
    const SOURCE: &str = r#"
const assert=require('assert'),bytes=new Uint8Array([0,255,128,65]),result=[];
for(const stream of [process.stdout,process.stderr]){
 for(const value of [Buffer.from(bytes),bytes,new Uint16Array(bytes.buffer),new DataView(bytes.buffer)])assert.strictEqual(stream.write(value),true);
 for(const value of [1,true,{},[]])try{stream.write(value);assert.fail('expected chunk error')}catch(error){result.push([error.name,error.code,error.message]);}
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
    assert_eq!(observations[0].stderr, observations[1].stderr);
}

#[cfg(unix)]
#[test]
fn synchronous_child_stdin_and_stdio_preserve_raw_views_and_encoded_units() {
    const SOURCE: &str = r#"
const assert=require('assert'),cp=require('child_process'),result=[];
const bytes=new Uint8Array([0,255,128,65]);
for(const input of [Buffer.from(bytes),bytes,new Uint16Array(bytes.buffer),new DataView(bytes.buffer),Buffer.alloc(256*1024,85)]){
 const child=cp.spawnSync('cat',[],{input,maxBuffer:1024*1024});assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);assert.strictEqual(child.stdout.length,input.byteLength);assert.strictEqual(child.stderr.length,0);result.push([child.stdout.length,child.stdout[0],child.stdout[child.stdout.length-1]]);
}
for(const [input,encoding] of [['hello','utf8'],['00ff80','hex'],['\ud800A','utf16le'],['😀','latin1']]){
 const child=cp.spawnSync('cat',[],{input,encoding});assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);result.push([encoding,child.stdout,child.stderr]);
}
const inherited=cp.spawnSync(process.execPath,['-e','process.stdout.write("forward");process.stderr.write("forwardERR")'],{stdio:'inherit',encoding:'hex'});assert.strictEqual(inherited.status,0);assert.strictEqual(inherited.error,undefined);assert.deepStrictEqual(inherited.output,[null,null,null]);assert.strictEqual(inherited.stdout,null);assert.strictEqual(inherited.stderr,null);
const ignored=cp.spawnSync(process.execPath,['-e','process.stdout.write("ok");process.stderr.write("ignored")'],{stdio:['ignore','pipe','ignore']});assert.strictEqual(ignored.stdout.toString(),'ok');assert.strictEqual(ignored.stderr,null);
for(const options of [{input:3},{stdio:['ipc','ipc','ipc']}])try{cp.spawnSync('cat',[],options);assert.fail('expected input/IPC error')}catch(error){result.push([error.name,error.code,error.message]);}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
    assert_eq!(observations[0].stderr, observations[1].stderr);
}

#[cfg(target_os = "linux")]
#[test]
fn synchronous_child_deadlines_reap_selected_signals_and_preserve_exit_handlers() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert'),os=require('os'),result=[];
const constants=os.constants.signals;assert.strictEqual(Object.getPrototypeOf(constants),null);assert.strictEqual(Object.getPrototypeOf(os.constants),null);assert.strictEqual(Object.isFrozen(constants),true);assert.strictEqual(Object.isExtensible(constants),false);assert.strictEqual(Reflect.set(constants,'NEW_SIGNAL',1),false);
result.push([Object.keys(constants),Object.getOwnPropertyDescriptor(constants,'SIGKILL')]);
for(const killSignal of [undefined,'sigkill',constants.SIGKILL,'SIGUSR1']){
 const child=cp.spawnSync('sleep',['5'],{timeout:50,killSignal});assert.strictEqual(child.error.code,'ETIMEDOUT');assert(child.error.errno<0);assert.strictEqual(child.status,null);result.push([child.error.code,child.error.errno,child.status,child.signal]);
}
const handled=cp.spawnSync(process.execPath,['-e','process.on("SIGTERM",()=>process.exit(7));setInterval(()=>{},1000)'],{timeout:300});assert.strictEqual(handled.error.code,'ETIMEDOUT');assert.strictEqual(handled.status,7);assert.strictEqual(handled.signal,null);result.push(['handled',handled.status]);
const inherited=cp.spawnSync('sleep',['5'],{timeout:50,stdio:'ignore',killSignal:'SIGKILL'});assert.strictEqual(inherited.error.code,'ETIMEDOUT');assert.strictEqual(inherited.signal,'SIGKILL');assert.deepStrictEqual(inherited.output,[null,null,null]);
for(const options of [{timeout:-1},{timeout:1.2},{timeout:'1'},{killSignal:true},{killSignal:0},{killSignal:'SIG_NO_SUCH_SIGNAL'}])try{cp.spawnSync('true',options);assert.fail('expected option validation error')}catch(error){result.push([error.name,error.code,error.message]);}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(target_os = "linux")]
#[test]
fn synchronous_child_timeout_closes_grandchild_pipes_after_direct_exit() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert');
const source='const cp=require("child_process");const child=cp.spawn("sleep",["5"],{stdio:["ignore","inherit","inherit"]});console.log(child.pid);child.unref();process.exit(0)';
const child=cp.spawnSync(process.execPath,['-e',source],{timeout:500});
const pid=Number(child.stdout.toString().trim());assert(pid>0);try{process.kill(pid,'SIGKILL')}catch(error){if(error.code!=='ESRCH')throw error;}
assert.strictEqual(child.error.code,'ETIMEDOUT');assert.strictEqual(child.status,0);assert.strictEqual(child.signal,null);console.log(JSON.stringify([child.error.code,child.status,child.signal]));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn child_options_snapshot_getters_once_and_validate_common_fields_in_native_code() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert'),result=[];
for(const spawn of [cp.spawn,cp.spawnSync]){
 for(const [name,value] of [['cwd',1],['cwd',true],['cwd',[]],['detached',1],['detached','yes'],['uid','0'],['uid',1.5],['uid',2147483648],['gid',false],['gid',NaN],['shell',1],['shell',[]],['argv0',1],['argv0','bad\0arg'],['windowsHide','yes'],['windowsVerbatimArguments',1],['timeout',-1],['killSignal',true]]){
  try{spawn('quench-oracle-never-launched',[],{[name]:value});assert.fail('expected option rejection')}catch(error){result.push([name,error.name,error.code,error.message]);}
 }
}
for(const serialization of [null,42,Infinity,'unsupported'])try{cp.spawn(process.execPath,[],{serialization});assert.fail('expected serialization rejection')}catch(error){result.push(['serialization',error.name,error.code,error.message]);}
const trace=[],options={};for(const [name,value] of [['cwd',process.cwd()],['detached',false],['uid',process.getuid()],['gid',process.getgid()],['argv0','oracle'],['windowsHide',false],['timeout',0]])Object.defineProperty(options,name,{enumerable:true,get(){trace.push(name);return value}});
const sync=cp.spawnSync(process.execPath,['-e','0'],options);assert.strictEqual(sync.status,0);assert.deepStrictEqual(trace,['cwd','detached','uid','gid','argv0','windowsHide','timeout']);result.push(trace);
const hidden={get detached(){Object.defineProperty(this,'uid',{enumerable:false});return false}};Object.defineProperty(hidden,'uid',{enumerable:true,configurable:true,get(){throw new Error('hidden getter must not run')}});assert.strictEqual(cp.spawnSync(process.execPath,['-e','0'],hidden).status,0);
const marker={};assert.throws(()=>cp.spawnSync(process.execPath,[],{get cwd(){throw marker}}),error=>error===marker);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(target_os = "linux")]
#[test]
fn child_identity_uses_kernel_credentials_sessions_and_spawn_error_boundaries() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert'),result=[];
assert.strictEqual(process.getgroups.name,'getgroups');assert.strictEqual(process.getgroups.length,0);result.push(process.getgroups());
const code='console.log(JSON.stringify([process.getuid(),process.getgid(),process.getgroups()]))';
const child=cp.spawnSync(process.execPath,['-e',code],{uid:process.getuid(),gid:process.getgid(),encoding:'utf8'});assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);result.push(JSON.parse(child.stdout));
for(const [name,value] of [['uid',0],['gid',0]]){
 if(name==='uid'?process.getuid()===0:process.getgroups().includes(0))continue;
 assert.throws(()=>cp.spawn('true',[],{[name]:value}),error=>{result.push([error.name,error.code,error.errno,error.message,error.syscall,Object.keys(error)]);return error.code==='EPERM'});
 const failed=cp.spawnSync('true',[],{[name]:value});assert.strictEqual(failed.error.code,'EPERM');assert.strictEqual(failed.status,null);result.push([failed.error.code,failed.error.errno,failed.error.syscall,failed.pid]);
}
const session='const cp=require("child_process");const sid=Number(cp.spawnSync("ps",["-o","sid=","-p",String(process.pid)],{encoding:"utf8"}).stdout.trim());console.log(sid===process.pid)';
for(const detached of [false,true]){const child=cp.spawnSync(process.execPath,['-e',session],{detached,encoding:'utf8'});assert.strictEqual(child.status,0);assert.strictEqual(child.stdout.trim(),String(detached));result.push(child.stdout.trim());}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn child_file_and_argument_null_validation_preserves_node_codes_and_messages() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert'),result=[];
for(const spawn of [cp.spawn,cp.spawnSync]){
 for(const file of [undefined,null,1,false,{},[],'','bad\0file'])try{spawn(file,[]);assert.fail('expected file rejection')}catch(error){result.push([error.name,error.code,error.message]);}
 try{spawn(process.execPath,['bad\0argument']);assert.fail('expected argument rejection')}catch(error){result.push([error.name,error.code,error.message]);}
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
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
fn child_disposal_and_sync_shell_options_observe_public_hooks_and_native_validation() {
    const SOURCE: &str = r#"
const assert=require('assert'),cp=require('child_process'),result=[];
const method=cp.ChildProcess.prototype[Symbol.dispose],descriptor=Object.getOwnPropertyDescriptor(cp.ChildProcess.prototype,Symbol.dispose),events=[];
result.push([method.name,method.length,Object.hasOwn(method,'prototype'),descriptor.writable,descriptor.enumerable,descriptor.configurable]);
const receiver={get killed(){events.push('killed');return false},get kill(){events.push('kill');return function(){events.push(this===receiver)}}};assert.strictEqual(method.call(receiver),undefined);result.push(events);
const marker={};assert.throws(()=>method.call({killed:false,kill(){throw marker}}),error=>error===marker);assert.strictEqual(method.call({killed:1,get kill(){throw marker}}),undefined);
for(const run of [cp.execSync,cp.execFileSync])try{run(process.execPath,{shell:'invalid\0shell'});assert.fail('expected shell rejection')}catch(error){result.push([error.name,error.code]);assert.strictEqual(error.code,'ERR_INVALID_ARG_VALUE');}
assert.strictEqual(cp.execSync('printf shell',{encoding:'utf8'}),'shell');assert.strictEqual(cp.execFileSync('printf',['direct'],{encoding:'utf8'}),'direct');
const child=cp.spawn('sleep',['5']);child.on('exit',(code,signal)=>{assert.strictEqual(code,null);assert.strictEqual(signal,'SIGTERM');console.log(JSON.stringify(result))});assert.strictEqual(child[Symbol.dispose](),undefined);assert.strictEqual(child.killed,true);assert.strictEqual(child[Symbol.dispose](),undefined);
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(unix)]
#[test]
fn child_process_reference_state_and_public_handle_hooks_match_node_liveness() {
    const SOURCE: &str = r#"
const cp=require('child_process'),assert=require('assert'),result=[];
for(const name of ['ref','unref']){
 const method=cp.ChildProcess.prototype[name],descriptor=Object.getOwnPropertyDescriptor(cp.ChildProcess.prototype,name),trace=[];
 result.push([method.name,method.length,Object.hasOwn(method,'prototype'),descriptor.enumerable]);
 const handle={[name](){trace.push(this===handle)}};const receiver={get _handle(){trace.push('get');return handle}};
 assert.strictEqual(method.call(receiver),undefined);assert.deepStrictEqual(trace,['get','get',true]);assert.strictEqual(method.call({}),undefined);
 const marker={};assert.throws(()=>method.call({_handle:{[name](){throw marker}}}),error=>error===marker);
}
for(const mode of ['unref','ref']){
 const source='const cp=require("child_process");const child=cp.spawn("sleep",["5"],{stdio:"ignore"});console.log(child.pid);child.unref();'+(mode==='ref'?'child.ref();':'');
 const child=cp.spawnSync(process.execPath,['-e',source],{timeout:500,encoding:'utf8'});const pid=Number(child.stdout.trim());assert(pid>0);try{process.kill(pid,'SIGKILL')}catch(error){if(error.code!=='ESRCH')throw error;}
 result.push([mode,child.status,child.signal,child.error?.code]);if(mode==='unref'){assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined)}else{assert.strictEqual(child.error.code,'ETIMEDOUT');}
}
const child=cp.spawn('sleep',['0.1'],{stdio:'ignore'}),handle=child._handle;assert.strictEqual(handle.hasRef(),true);assert.strictEqual(child.unref(),undefined);assert.strictEqual(handle.hasRef(),false);assert.strictEqual(child.ref(),undefined);assert.strictEqual(handle.hasRef(),true);
child.on('exit',()=>{assert.strictEqual(child._handle,null);assert.strictEqual(handle.hasRef(),true);assert.strictEqual(child.ref(),undefined)});child.on('close',()=>{assert.strictEqual(handle.hasRef(),true);setTimeout(()=>{assert.strictEqual(handle.hasRef(),undefined);console.log(JSON.stringify(result))},10)});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(unix)]
#[test]
fn local_socket_transport_exchanges_raw_bytes_with_an_external_process_and_unlinks_listener() {
    const SOURCE: &str = r#"
const net=require('net'),fs=require('fs'),cp=require('child_process'),assert=require('assert');
const path='/tmp/quench-local-'+process.pid+'.sock';
const server=net.createServer(socket=>{const chunks=[];socket.on('data',chunk=>chunks.push(chunk));socket.on('end',()=>{assert.deepStrictEqual(Buffer.concat(chunks),Buffer.from([0,255,128,65]));socket.end(Buffer.from([1,254,127,66]));});});
server.listen(path,()=>{
 assert.strictEqual(server.address(),path);assert.strictEqual(fs.existsSync(path),true);
 const child=cp.spawn('python3',['-c','import socket,sys;s=socket.socket(socket.AF_UNIX);s.connect(sys.argv[1]);s.sendall(bytes([0,255,128,65]));s.shutdown(socket.SHUT_WR);out=b""\nwhile True:\n data=s.recv(1024)\n if not data:break\n out+=data\nassert out==bytes([1,254,127,66]);s.close()',path],{stdio:'inherit'});
 child.on('exit',code=>{assert.strictEqual(code,0);server.close(()=>{assert.strictEqual(fs.existsSync(path),false);console.log('external local bytes and unlink')});});
});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(unix)]
#[test]
fn local_socket_path_overloads_and_os_connection_errors_are_observable() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),fs=require('fs');const path='/tmp/quench-local-errors-'+process.pid+'.sock';
const missing=net.connect(path);missing.on('error',error=>{assert.strictEqual(error.code,'ENOENT');assert.strictEqual(error.errno,-2);assert.strictEqual(error.syscall,'connect');assert.strictEqual(error.address,path);});missing.on('close',()=>{
 const server=net.createServer(socket=>socket.end('ok'));server.listen({path},()=>{assert.strictEqual(server.address(),path);const socket=net.connect({path},()=>{assert.strictEqual(socket.connecting,false)});let output='';socket.setEncoding('utf8');socket.on('data',data=>output+=data);socket.on('end',()=>{assert.strictEqual(output,'ok');socket.end()});socket.on('close',()=>server.close(()=>{assert.strictEqual(fs.existsSync(path),false);console.log('local overload and actual ENOENT')}));});
});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(unix)]
#[test]
fn local_socket_bind_errors_are_async_and_long_paths_preserve_os_errno() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert');const path='/tmp/quench-local-boundary-'+process.pid+'.sock';
const first=net.createServer();first.listen(path,()=>{
 const second=net.createServer();let returned=false;second.on('error',error=>{assert.strictEqual(returned,true);assert.strictEqual(error.code,'EADDRINUSE');assert.strictEqual(error.syscall,'listen');assert.strictEqual(error.address,path);assert.strictEqual(error.port,-1);first.close(()=>{
  const long='/tmp/'+ 'x'.repeat(10000);const server=net.createServer();server.on('error',error=>{assert.strictEqual(error.code,'EINVAL');net.connect(long).on('error',error=>{assert.strictEqual(error.code,'EINVAL');console.log('actual bind conflict and path boundary')})});server.listen(long);
 });});second.listen(path);returned=true;
});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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

#[cfg(target_os = "linux")]
#[test]
fn local_abstract_socket_uses_kernel_namespace_and_releases_on_close() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert');const path='\0quench-abstract-'+process.pid;
const server=net.createServer(socket=>socket.end('abstract'));server.listen(path,()=>{assert.strictEqual(server.address(),path);const client=net.connect(path);let result='';client.setEncoding('utf8');client.on('data',chunk=>result+=chunk);client.on('end',()=>{assert.strictEqual(result,'abstract');client.end()});client.on('close',()=>server.close(()=>{const again=net.createServer();again.listen(path,()=>again.close(()=>console.log('kernel abstract close and rebind')))}));});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn net_endpoint_invalid_options_preserve_node_error_codes_and_messages() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),result=[];
for(const value of [false,true,{port:false},{port:true},{path:-1},{},{host:'localhost'}]) {
 try {net.createServer().listen(value);assert.fail('expected options rejection')}catch(error){result.push([error.name,error.code,error.message]);}
}
for(const value of [-1,NaN,Infinity,-Infinity,1.5,65536,'65536','']) {
 for(const connect of [false,true])try{if(connect)net.connect({port:value});else net.createServer().listen({port:value});assert.fail('expected port rejection')}catch(error){result.push([error.name,error.code,error.message]);}
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn net_server_reference_requests_preserve_public_hooks_and_real_listener_liveness() {
    const SOURCE: &str = r#"
const net=require('net'),cp=require('child_process'),assert=require('assert'),result=[];
for(const name of ['ref','unref']) {
 const method=net.Server.prototype[name],descriptor=Object.getOwnPropertyDescriptor(net.Server.prototype,name),trace=[];
 result.push([method.name,method.length,Object.hasOwn(method,'prototype'),descriptor.enumerable]);assert.throws(()=>method(),{name:'TypeError'});
 const handle={[name](){trace.push(this===handle)}},receiver={set _unref(value){trace.push(value)},get _handle(){trace.push('get');return handle}};
 assert.strictEqual(method.call(receiver),receiver);assert.deepStrictEqual(trace,[name==='unref','get','get',true]);
 const marker={};assert.throws(()=>method.call({_handle:{[name](){throw marker}}}),error=>error===marker);
}
for(const mode of ['before','after','ref']) {
 const source='const net=require("net");const server=net.createServer();'+(mode==='before'?'server.unref();':'')+'server.listen(0,()=>console.log("listening"));'+(mode==='after'?'server.unref();':'')+(mode==='ref'?'server.unref();server.ref();':'');
 const child=cp.spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:1500});assert.strictEqual(child.stdout,'listening\n');
 if(mode==='ref'){assert.strictEqual(child.error.code,'ETIMEDOUT');result.push([mode,child.error.code])}else{assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);result.push([mode,child.status])}
}
const server=net.createServer();assert.strictEqual(server._handle,null);assert.strictEqual(server._unref,false);assert.strictEqual(server.unref(),server);assert.strictEqual(server._unref,true);
server.listen(0,()=>{const handle=server._handle;assert.strictEqual(handle.hasRef(),false);assert.strictEqual(server.ref(),server);assert.strictEqual(handle.hasRef(),true);server.close(()=>{assert.strictEqual(server._handle,null);assert.strictEqual(handle.hasRef(),true);setTimeout(()=>{assert.strictEqual(handle.hasRef(),undefined);console.log(JSON.stringify(result))},10)});assert.strictEqual(handle.hasRef(),true);assert.strictEqual(server._handle,null);});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
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
fn net_socket_reference_requests_preserve_public_hooks_and_real_socket_liveness() {
    const SOURCE: &str = r#"
const net=require('net'),cp=require('child_process'),assert=require('assert'),result=[];
for(const name of ['ref','unref']) {
 const method=net.Socket.prototype[name],descriptor=Object.getOwnPropertyDescriptor(net.Socket.prototype,name),trace=[];
 result.push([method.name,method.length,Object.hasOwn(method,'prototype'),descriptor.enumerable]);assert.throws(()=>method(),{name:'TypeError'});
 const handle={get [name](){trace.push('method');return function(){trace.push(this===handle)}}},receiver={get _handle(){trace.push('handle');return handle}};
 assert.strictEqual(method.call(receiver),receiver);assert.deepStrictEqual(trace,['handle','handle','method','handle','method',true]);
 const callback=function(){},deferred={_handle:null,[name]:callback,once(event,fn){assert.strictEqual(this,deferred);assert.strictEqual(event,'connect');assert.strictEqual(fn,callback);trace.push('deferred')}};
 assert.strictEqual(method.call(deferred),deferred);assert.strictEqual(trace.pop(),'deferred');
 const marker={};assert.throws(()=>method.call({_handle:{[name](){throw marker}}}),error=>error===marker);
 assert.strictEqual(method.call({_handle:{[name]:1}})._handle[name],1);
}
for(const mode of ['before','after','ref']) {
 const source='const net=require("net");const server=net.createServer(socket=>socket.unref());server.unref();server.listen(0,()=>{const socket=new net.Socket();'+(mode==='before'?'socket.unref();':'')+'socket.connect(server.address().port);socket.once("connect",()=>{console.log("connected");socket.setTimeout(3000,()=>{throw new Error("unexpected socket timeout")});'+(mode!=='before'?'socket.unref();':'')+(mode==='ref'?'socket.ref();':'')+'});});';
 const child=cp.spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:1500});assert.strictEqual(child.stdout,'connected\n');
 if(mode==='ref'){assert.strictEqual(child.error.code,'ETIMEDOUT');result.push([mode,child.error.code])}else{assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);result.push([mode,child.status])}
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
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
fn net_option_validation_preserves_coded_prototypes_original_getters_and_messages() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),util=require('util'),result=[],prototypes=new Map(),brands=[];
function capture(action){try{const socket=action();socket.on('error',()=>{});socket.destroy();result.push('ok')}catch(error){
 assert(error instanceof TypeError);assert(util.types.isNativeError(error));const prototype=Object.getPrototypeOf(error);
 const keys=Reflect.ownKeys(prototype).map(String),kind=error.code+keys.join(',');if(prototypes.has(kind))assert.strictEqual(prototypes.get(kind),prototype);else prototypes.set(kind,prototype);
 let owner=prototype,symbol;while(owner&&!symbol){symbol=Object.getOwnPropertySymbols(owner).find(symbol=>symbol.description==='kIsNodeError');owner=Object.getPrototypeOf(owner)}assert.strictEqual(error[symbol],true);brands.push(symbol);result.push(keys);
 result.push([error.name,error.code,error.message,error.toString(),Object.keys(error),prototype.constructor===TypeError]);
}}
for(const value of [undefined,null,false,true,{}, {port:undefined},{path:undefined},{path:null},{path:false},{port:null},{port:0,host:[]},{port:0,host:'a\0b'},{port:0,lookup:1},{port:0,lookup:null}])capture(()=>net.connect(value));
for(const method of [()=>net.connect(),()=>new net.Socket().connect(),()=>new net.Socket().connect({})])capture(method);
for(const key of ['objectMode','readableObjectMode','writableObjectMode'])for(const value of [true,1,'x',{},false])capture(()=>new net.Socket({[key]:value}));
for(const key of ['objectMode','readableObjectMode','writableObjectMode'])capture(()=>net.connect({port:0,[key]:true}));
assert(brands.every(symbol=>symbol===brands[0]));
const trace=[];capture(()=>new net.Socket({get objectMode(){trace.push('object');return false},get readableObjectMode(){trace.push('readable');return 1},get writableObjectMode(){trace.push('writable');return 2}}));result.push(trace);
const marker={};assert.throws(()=>new net.Socket({get objectMode(){throw marker}}),error=>error===marker);
let lookupReads=0;capture(()=>net.connect({port:0,host:'localhost',get lookup(){lookupReads++;return lookupReads===1?function(){}:1}}));result.push(lookupReads);
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
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
fn net_lookup_callbacks_preserve_original_errors_order_metadata_and_preconnect_bytes() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),cp=require('child_process'),result=[];
(async()=>{
for(const family of [undefined,4,6])for(const kind of ['original','ip','family'])await new Promise((resolve,reject)=>{
 const socket=new net.Socket(),trace=[],marker=Object.assign(new Error('original lookup failure'),{code:'ORIGINAL',errno:'original',syscall:'lookup-owner'});
 socket.on('lookup',(error,ip,type,host)=>trace.push(['lookup',error===marker,ip,type,host]));
 socket.on('connect',()=>reject(new Error('unexpected connect')));
 socket.on('error',error=>{try{
  if(kind==='original')assert.strictEqual(error,marker);
  else assert.strictEqual(error.code,kind==='ip'?'ERR_INVALID_IP_ADDRESS':'ERR_INVALID_ADDRESS_FAMILY');
  result.push([family,kind,trace,error.name,error.code,error.message,error.toString(),Object.keys(error),error.host,error.port]);resolve();
 }catch(error){reject(error)}});
 socket.connect({port:0,host:'lookup.invalid',family,lookup(host,options,callback){
  trace.push(['request',host,options.family,options.hints,options.all,callback.name,callback.length,Object.hasOwn(callback,'prototype')]);
  if(kind==='original')callback(marker);
  else if(options.all)callback(null,[{address:kind==='ip'?1:'127.0.0.1',family:kind==='family'?100:4}]);
  else callback(null,kind==='ip'?['127.0.0.1']:'127.0.0.1',kind==='family'?100:4);
  trace.push(['returned']);
 }});
 trace.push(['connected-call-returned']);
});
await new Promise((resolve,reject)=>{
 const server=net.createServer(peer=>peer.on('data',chunk=>{peer.end(chunk)}));server.on('error',reject);
 server.listen(0,'127.0.0.1',()=>{
  const trace=[],socket=net.connect({port:server.address().port,host:'custom.lookup',family:4,lookup(host,options,callback){trace.push([host,options.family,options.hints,options.all]);process.nextTick(()=>callback(null,'127.0.0.1',4))}});
  let bytes='';socket.on('error',reject);socket.on('data',chunk=>bytes+=chunk.toString('hex'));
  socket.on('end',()=>{try{assert.strictEqual(bytes,'0080ff');result.push(['bytes',bytes,trace]);server.close(resolve)}catch(error){reject(error)}});
  socket.write(Buffer.from([0,128,255]));
 });
});
const source='const net=require("net");const socket=net.connect({port:0,host:"custom.lookup",lookup(host,options,callback){console.log(callback.name,callback.length,Object.hasOwn(callback,"prototype"));}});console.log(socket._handle.hasRef());';
const child=cp.spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:2000});assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);result.push(['inactive',child.stdout]);
console.log(JSON.stringify(result));
})().catch(error=>{console.error(error);process.exitCode=1});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(20)).unwrap()
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
fn net_ip_predicates_preserve_original_coercion_exceptions_and_zone_syntax() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),result=[];
for(const name of ['isIP','isIPv4','isIPv6']){
 const method=net[name],descriptor=Object.getOwnPropertyDescriptor(method,'name');result.push([method.name,method.length,Object.hasOwn(method,'prototype'),descriptor.writable,descriptor.enumerable,descriptor.configurable]);
 const marker={};assert.throws(()=>net[name]({[Symbol.toPrimitive](){throw marker}}),error=>error===marker);
 assert.throws(()=>net[name](Symbol('ip')),{name:'TypeError'});
 for(const text of ['127.0.0.1','::1','bad','fe80::1%eth_0','fe80::1%eth:0','fe80::1%eth-0','fe80::1%eth.0']){
  let calls=0;const value={[Symbol.toPrimitive](hint){assert.strictEqual(hint,'string');calls++;return text}};
  result.push([name,text,net[name](value),calls]);
 }
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn tcp_connect_failures_preserve_os_fields_property_order_and_intrinsic_error() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),OriginalError=Error;
const port=Number(process.argv[1]),host=process.argv[2],result=[];
globalThis.Error=function(){throw new OriginalError('poisoned Error constructor')};
const socket=net.connect({host,port,autoSelectFamily:false});
socket.on('connect',()=>{throw new OriginalError('unexpected connection')});
socket.on('error',error=>{
 assert(error instanceof OriginalError);assert.strictEqual(error.code,'ECONNREFUSED');assert.strictEqual(error.address,host);assert.strictEqual(error.port,port);assert.strictEqual(error.syscall,'connect');
 console.log(JSON.stringify([error.name,error.code,error.errno,error.syscall,error.address,error.port,error.message,error.toString(),Object.keys(error),Object.getPrototypeOf(error).constructor===OriginalError]));
});
"#;
    for host in ["127.0.0.1", "::1"] {
        let listener = match std::net::TcpListener::bind((host, 0)) {
            Ok(listener) => listener,
            Err(_) if host == "::1" => continue,
            Err(error) => panic!("cannot reserve oracle port: {error}"),
        };
        let port = listener.local_addr().unwrap().port().to_string();
        drop(listener);
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.args(["-e", SOURCE, &port, host]);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(15))
                .unwrap()
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
}

#[test]
fn net_socket_close_completion_follows_ticks_and_releases_captured_handle_reference() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),result=[];
(async()=>{
for(const hadError of [false,true])await new Promise((resolve,reject)=>{
 const server=net.createServer(peer=>peer.resume());server.on('error',reject);
 server.listen(0,'127.0.0.1',()=>{
  const socket=net.connect(server.address().port,'127.0.0.1');socket.on('error',()=>{});
  socket.once('connect',()=>{
   const handle=socket._handle,trace=[['open',handle.hasRef()]];let manual=true;
   socket.on('close',error=>{
    if(manual){assert.strictEqual(handle.hasRef(),true);trace.push(['manual',error]);return}
    try{trace.push(['close',handle.hasRef(),error]);assert.strictEqual(handle.hasRef(),false);handle.ref();assert.strictEqual(handle.hasRef(),false);result.push([hadError,trace]);server.close(resolve)}catch(error){reject(error)}
   });
   socket.emit('close',false);manual=false;
   socket.destroy(hadError?new Error('socket failure'):undefined);
   trace.push(['destroy',handle.hasRef(),socket._handle===null]);
   process.nextTick(()=>trace.push(['tick',handle.hasRef()]));
  });
 });
});
for(const hadError of [false,true])await new Promise(resolve=>{
 const socket=new net.Socket(),trace=[];socket.on('error',()=>trace.push('error'));
 socket.on('close',error=>{trace.push(['close',error]);result.push(['nohandle',hadError,trace]);resolve()});
 socket.destroy(hadError?new Error('unconnected failure'):undefined);trace.push('returned');
});
console.log(JSON.stringify(result));
})().catch(error=>{console.error(error);process.exitCode=1});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
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
fn stream_emit_close_option_suppresses_only_close_and_preserves_destroy_errors() {
    const SOURCE: &str = r#"
const stream=require('stream'),assert=require('assert'),result=[];
(async()=>{
for(const name of ['Readable','Writable','Duplex'])for(const emitClose of [true,false])await new Promise((resolve,reject)=>{
 const marker=new Error('destroy marker'),trace=[],value=new stream[name]({emitClose,read(){},write(chunk,encoding,callback){callback()}});
 value.on('error',error=>{assert.strictEqual(error,marker);trace.push('error')});value.on('close',()=>trace.push('close'));
 value.destroy(marker);trace.push('returned');setImmediate(()=>setImmediate(()=>{try{assert.deepStrictEqual(trace,emitClose?['returned','error','close']:['returned','error']);result.push([name,emitClose,trace,value.closed,value._readableState?.closeEmitted,value._writableState?.closeEmitted]);resolve()}catch(error){reject(error)}}));
});
console.log(JSON.stringify(result));
})().catch(error=>{console.error(error);process.exitCode=1});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
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
fn net_family_attempt_timeout_matches_node_ranges_brands_and_defaults() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),result=[];
result.push(['default',net.getDefaultAutoSelectFamilyAttemptTimeout()]);
for(const value of [undefined,null,false,'1',{},Symbol('timeout'),1n,NaN,Infinity,-Infinity,-0,-1,1.5,2147483648,4294967297]) {
  for(const connect of [false,true]) {
    if(connect&&(value===undefined||value===null)) continue;
    try {
      if(connect) net.connect({host:'127.0.0.1',port:1,autoSelectFamilyAttemptTimeout:value});
      else net.setDefaultAutoSelectFamilyAttemptTimeout(value);
      assert.fail('expected rejection');
    } catch(error) {
      assert(error instanceof TypeError || error instanceof RangeError);
      const prototype=Object.getPrototypeOf(error),base=Object.getPrototypeOf(prototype);
      result.push([connect,error.name,error.code,error.message,String(error),Object.keys(error),Object.hasOwn(prototype,'constructor'),Object.hasOwn(base,'constructor')]);
    }
  }
}
for(const value of [1,9,10,500,2147483647]) {
  assert.strictEqual(net.setDefaultAutoSelectFamilyAttemptTimeout(value),undefined);
  result.push([value,net.getDefaultAutoSelectFamilyAttemptTimeout()]);
}
for(const host of ['127.0.0.1','validation.invalid']) {
  const marker={},socket=new net.Socket(),trace=[];
  assert.throws(()=>socket.connect({host,port:1,get autoSelectFamilyAttemptTimeout(){trace.push(socket._handle!==null,socket.connecting);throw marker}}),error=>error===marker);
  result.push(['getter',host,trace]);socket.destroy();
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn net_family_attempt_timeout_cli_preserves_value_and_last_override() {
    const SOURCE: &str = "console.log(require('net').getDefaultAutoSelectFamilyAttemptTimeout())";
    for flags in [
        vec!["--network-family-autoselection-attempt-timeout=1"],
        vec!["--network-family-autoselection-attempt-timeout=123"],
        vec![
            "--network-family-autoselection-attempt-timeout=123",
            "--network-family-autoselection-attempt-timeout=456",
        ],
    ] {
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.args(&flags).args(["-e", SOURCE]);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(10))
                .unwrap()
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
}
#[test]
fn buffer_string_encoding_uses_intrinsic_bytes_without_guest_iterator_hooks() {
    const SOURCE: &str = r#"
const {Buffer}=require('buffer'),assert=require('assert'),result=[];
const originalArrayIterator=Array.prototype[Symbol.iterator],originalTypedIterator=Uint8Array.prototype[Symbol.iterator],OriginalUint8Array=Uint8Array;
let arrayCalls=0,typedCalls=0,constructorCalls=0;
const poisonArray=function(){arrayCalls++;throw new Error('array iterator consulted')};
const poisonTyped=function(){typedCalls++;throw new Error('typed iterator consulted')};
const poisonConstructor=function(){constructorCalls++;throw new Error('global Uint8Array consulted')};
const inputs=[['007f80ff','hex'],['007f80ffx','hex'],['a','hex'],['hé🙂\ud800','utf8'],['\x00\x7f\x80\xff','latin1'],['AH+A/w==','base64']];
for(let i=0;i<inputs.length;i++) {
 let buffer;
 try {
  Array.prototype[Symbol.iterator]=poisonArray;
  OriginalUint8Array.prototype[Symbol.iterator]=poisonTyped;
  globalThis.Uint8Array=poisonConstructor;
  buffer=Buffer.from(inputs[i][0],inputs[i][1]);
 } finally {
  Array.prototype[Symbol.iterator]=originalArrayIterator;
  OriginalUint8Array.prototype[Symbol.iterator]=originalTypedIterator;
  globalThis.Uint8Array=OriginalUint8Array;
 }
 assert(Buffer.isBuffer(buffer));assert.strictEqual(Object.getPrototypeOf(buffer),Buffer.prototype);
 if(typeof globalThis.gc==='function') globalThis.gc();
 const copy=Buffer.from(inputs[i][0],inputs[i][1]);
 if(buffer.length){const byte=copy[0];buffer[0]^=255;assert.strictEqual(copy[0],byte);buffer[0]^=255;}
 result.push([inputs[i][1],buffer.toString('hex'),buffer.length]);
}
assert.strictEqual(arrayCalls,0);assert.strictEqual(typedCalls,0);assert.strictEqual(constructorCalls,0);
console.log(JSON.stringify([result,arrayCalls,typedCalls,constructorCalls]));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn buffer_private_encoding_does_not_consult_typed_length_or_subarray_hooks() {
    const SOURCE: &str = r#"
const {Buffer}=require('buffer'),assert=require('assert'),result=[];
const originalLength=Object.getOwnPropertyDescriptor(Uint8Array.prototype,'length'),originalSubarray=Uint8Array.prototype.subarray,marker={};
let length,encoded;
try {
 Object.defineProperty(Uint8Array.prototype,'length',{configurable:true,get(){throw marker}});
 encoded=Buffer.from('hé','utf8');
 length=Buffer.byteLength('hé','utf8');
} finally {
 if(originalLength)Object.defineProperty(Uint8Array.prototype,'length',originalLength);else delete Uint8Array.prototype.length;
}
assert.strictEqual(length,3);result.push(length,encoded.toString('hex'));
const target=Buffer.alloc(8);
let written;
try {
 Uint8Array.prototype.subarray=function(){throw marker};
 written=target.write('abc');
} finally {
 Uint8Array.prototype.subarray=originalSubarray;
}
assert.strictEqual(written,3);result.push(written,target.toString('hex'));
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn fatal_promise_rejections_preserve_original_stack_and_intrinsic_wrapper() {
    for mode in ["throw", "strict"] {
        for primitive in [false, true] {
            let source = if primitive {
                "globalThis.Error=function(){throw 'constructor-poison'};Promise.reject('primitive-rejection-marker');"
            } else {
                "const error=new Error('original-rejection-marker');error.stack='Error: original-rejection-marker\\n    at preserved-origin';globalThis.Error=function(){throw 'constructor-poison'};Promise.reject(error);"
            };
            for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
                let mut command = Command::new(executable);
                command.arg(format!("--unhandled-rejections={mode}")).args([
                    "--no-warnings",
                    "-e",
                    source,
                ]);
                if executable != "node" {
                    command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
                }
                let observation = quench_node_test::case_process::observe_command(
                    command,
                    Duration::from_secs(10),
                )
                .unwrap();
                assert!(!observation.timed_out);
                assert_eq!(observation.exit_code, Some(1));
                assert_eq!(observation.signal, None);
                assert!(observation.stdout.is_empty());
                let stderr = String::from_utf8_lossy(&observation.stderr);
                let marker = if primitive {
                    "primitive-rejection-marker"
                } else {
                    "original-rejection-marker"
                };
                assert!(stderr.contains(marker), "{executable}: {stderr}");
                assert!(
                    !stderr.contains("constructor-poison"),
                    "{executable}: {stderr}"
                );
                if !primitive {
                    assert!(
                        stderr.contains("at preserved-origin"),
                        "{executable}: {stderr}"
                    );
                }
            }
        }
    }
    const SOURCE: &str = r#"
const OriginalError=Error,assert=require('assert');
globalThis.Error=function(){throw 'constructor-poison'};
process.on('uncaughtException',(error,origin)=>{
 assert(error instanceof OriginalError);
 assert.strictEqual(origin,'unhandledRejection');
 console.log(JSON.stringify([error.name,error.code,error.message,Object.keys(error)]));
});
Promise.reject('primitive-rejection-marker');
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args([
            "--unhandled-rejections=throw",
            "--no-warnings",
            "-e",
            SOURCE,
        ]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn net_server_constructor_rejects_primitive_options_with_direct_node_errors() {
    const SOURCE: &str = r#"
const net=require('net'),assert=require('assert'),result=[];
for(const value of ['path','',0,1,NaN,true,false,Symbol('options'),1n]) {
 for(const invoke of [value=>net.createServer(value),value=>new net.Server(value),value=>net.Server(value)]) {
  try {invoke(value);assert.fail('expected options rejection')}catch(error){
   assert(error instanceof TypeError);assert.strictEqual(error.code,'ERR_INVALID_ARG_TYPE');
   result.push([error.name,error.code,error.message,String(error),Object.keys(error),Object.hasOwn(Object.getPrototypeOf(error),'constructor')]);
  }
 }
}
for(const value of [undefined,null,{},[],function connection(){}]) {
 const server=net.createServer(value);assert(server instanceof net.Server);assert.strictEqual(server._handle,null);
}
console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn native_fs_watch_matches_node_events_close_references_and_abort_lifecycle() {
    const SOURCE: &str = r#"
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-watch-oracle-')),file=path.join(dir,'watched'),result=[];
fs.writeFileSync(file,'before');
const a=fs.watch(file),b=fs.watch(file,{encoding:'buffer'}),ha=a._handle;
result.push(['meta',a.constructor.name,Object.keys(a),Object.keys(ha),['close','ref','unref'].map(k=>[a[k].name,a[k].length,Object.hasOwn(a[k],'prototype')])]);
assert.strictEqual(a.ref(),a);assert.strictEqual(a.unref(),a);assert.strictEqual(ha.hasRef(),false);a.ref();assert.strictEqual(ha.hasRef(),true);
let seenA=false,seenB=false;
a.on('change',(kind,name)=>{if(seenA)return;seenA=true;result.push(['a',kind,name]);a.close();assert.strictEqual(a._handle,null);assert.strictEqual(ha.hasRef(),true);assert.strictEqual(a.close(),undefined);});
b.on('change',(kind,name)=>{if(seenB)return;seenB=true;assert(Buffer.isBuffer(name));result.push(['b',kind,name.toString()]);b.close();});
let closed=0;
for(const watcher of [a,b])watcher.on('close',()=>{closed++;if(closed===2){assert(seenA&&seenB);const ac=new AbortController();const aborted=fs.watch(file,{signal:ac.signal});aborted.on('close',()=>{result.push(['abort',aborted._handle]);const pre=fs.watch(file,{signal:AbortSignal.abort()});pre.on('close',()=>{result.push(['preabort',pre._handle]);fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));});});ac.abort();}});
fs.writeFileSync(file,'after');
setTimeout(()=>{assert.strictEqual(closed,2);},100);
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn native_fs_watch_missing_path_and_unreferenced_exit_match_node() {
    const SOURCE: &str = r#"
const fs=require('fs'),cp=require('child_process'),path=require('path'),os=require('os'),assert=require('assert'),result=[];
try{fs.watch('quench-watch-definitely-missing');assert.fail('expected error')}catch(error){result.push([error.message,error.errno,error.code,error.syscall,error.path,error.filename,Object.keys(error)]);}
const dormant=fs.watch('quench-watch-definitely-missing',{throwIfNoEntry:false});assert.strictEqual(dormant._handle.initialized,false);assert.strictEqual(dormant.close(),undefined);assert(dormant._handle);dormant.unref();
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-watch-exit-')),file=path.join(dir,'watched');fs.writeFileSync(file,'data');
for(const action of ['options','unref','restore']){
 const source='const fs=require("fs");const watcher=fs.watch('+JSON.stringify(file)+','+(action==='options'?'{persistent:false}':'{}')+');'+(action==='options'?'':'watcher.unref();')+(action==='restore'?'watcher.ref();':'')+'console.log("ready");';
 const child=cp.spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:1000});assert.strictEqual(child.stdout,'ready\n');
 if(action==='restore'){assert.strictEqual(child.error.code,'ETIMEDOUT');result.push([action,child.error.code]);}else{assert.strictEqual(child.status,0);assert.strictEqual(child.error,undefined);result.push([action,child.status]);}
}
fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn native_fs_watch_ignore_validation_and_filters_match_node() {
    const SOURCE: &str = r#"
const fs=require('fs'),assert=require('assert'),path=require('path'),os=require('os'),result=[];
for(const ignore of [123,'',[123],['']]){
 try{fs.watch('.',{ignore});assert.fail('expected rejection')}catch(error){result.push([error.name,error.code,error.message,Object.keys(error)]);}
}
for(const encoding of [true,1,Symbol('encoding'),'invalid']){
 try{fs.watch('.',{encoding});assert.fail('expected rejection')}catch(error){result.push([error.name,error.code,error.message]);}
}
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-watch-ignore-'));
const trace=[],re=/\.tmp$/;re.exec=()=>{throw new Error('public exec consulted')};
const watcher=fs.watch(dir,{ignore:['*.log',re,function(name){trace.push([Array.isArray(this),this.length]);return name.startsWith('.')} ]});
watcher.on('change',(kind,name)=>{
 assert.strictEqual(name,'keep.txt');watcher.close();
});
watcher.on('close',()=>{assert(trace.length>0);assert(trace.every(row=>row[0]&&row[1]===3));result.push(['kept',trace[trace.length-1]]);fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(result));});
for(const name of ['ignored.log','ignored.tmp','.hidden','keep.txt'])fs.writeFileSync(path.join(dir,name),'data');
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn native_fs_watch_inherited_options_and_encoding_getters_match_node() {
    const SOURCE: &str = r#"
const fs=require('fs'),assert=require('assert'),path=require('path'),os=require('os'),result=[];
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-watch-options-')),file=path.join(dir,'watched');fs.writeFileSync(file,'data');
for(const encoding of ['utf8','buffer']){
 const trace=[],proto={get encoding(){trace.push('encoding');return encoding},get signal(){trace.push('signal');return undefined},persistent:false};
 const options=Object.create(proto);const marker={};Object.defineProperty(options,Symbol('ignored'),{enumerable:true,get(){throw marker}});
 const watcher=fs.watch(file,options);result.push([encoding,trace,watcher._handle.hasRef()]);watcher.close();
}
for(const encoding of ['',false,0,null]){const watcher=fs.watch(file,{encoding,persistent:false});watcher.close();}
const options=Object.create({persistent:false});Object.defineProperty(options,'persistent',{value:false,enumerable:false});const watcher=fs.watch(file,options);result.push(['shadow',watcher._handle.hasRef()]);watcher.close();
fs.rmSync(dir,{recursive:true,force:true});process.nextTick(()=>console.log(JSON.stringify(result)));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn native_fs_watch_keeps_roots_across_gc_and_routes_original_callback_errors() {
    const SOURCE: &str = r#"
const fs=require('fs'),assert=require('assert'),path=require('path'),os=require('os');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-watch-errors-')),file=path.join(dir,'watched');fs.writeFileSync(file,'before');
const marker={},result=[];
let watcher=fs.watch(file),handle=watcher._handle;
watcher.on('change',()=>{globalThis.gc();throw marker});
process.once('uncaughtException',(reason,origin)=>{
 assert.strictEqual(reason,marker);assert.strictEqual(origin,'uncaughtException');watcher.close();
 result.push(['listener',handle.hasRef()]);
 watcher.on('close',()=>{
  globalThis.gc();const filterMarker={};const filtered=fs.watch(file,{ignore(){globalThis.gc();throw filterMarker}});
  process.once('uncaughtException',(reason,origin)=>{assert.strictEqual(reason,filterMarker);assert.strictEqual(origin,'uncaughtException');filtered.close();filtered.on('close',()=>{fs.rmSync(dir,{recursive:true,force:true});result.push(['filter']);console.log(JSON.stringify(result));});});
  fs.writeFileSync(file,'filter');
 });
});
globalThis.gc();fs.writeFileSync(file,'after');
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
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
fn native_tls_context_versions_constructors_and_weak_gc_match_node() {
    const SOURCE: &str = r#"
const tls=require('tls'),assert=require('assert');
console.log(JSON.stringify([tls.SecureContext.name,tls.SecureContext.length,tls.createSecureContext.name,tls.createSecureContext.length]));
for(const options of [undefined,{minVersion:'TLSv1',maxVersion:'TLSv1.2'},{minVersion:'TLSv1.3',maxVersion:'TLSv1.3'},{secureProtocol:'TLS_method'},{secureProtocol:'SSLv23_method'},{secureProtocol:'TLSv1_1_client_method'}]){
 const context=tls.createSecureContext(options);assert(context instanceof tls.SecureContext);assert(!(context.context instanceof tls.SecureContext));global.gc();console.log(JSON.stringify([Object.keys(context),context.context.constructor.name,context.context.getMinProto(),context.context.getMaxProto(),context.context.getCertificate()]));
}
const plain=tls.SecureContext(),plainMinimum=plain.context.getMinProto();assert(plain instanceof tls.SecureContext);const receiver=Object.create(tls.SecureContext.prototype);assert.strictEqual(tls.SecureContext.call(receiver),undefined);console.log(receiver.context.getMinProto(),receiver.context.getMaxProto());
const before=tls.DEFAULT_MIN_VERSION;tls.DEFAULT_MIN_VERSION='TLSv1.1';console.log(tls.createSecureContext().context.getMinProto());tls.DEFAULT_MIN_VERSION=before;
for(let i=0;i<100;i++)tls.createSecureContext();global.gc();console.log('contexts-survive-gc');
process.on('exit',()=>{assert.strictEqual(plain.context.getMinProto(),plainMinimum);assert.strictEqual(tls.createSecureContext().context.getCertificate(),null);console.log('exit-contexts-survive');});
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-gc", "-e", SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(
            observation.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}


#[test]
fn native_tls_context_protocol_options_errors_match_node() {
    compare_node_stream_source(
        r#"
const tls=require('tls');const forms=[{},1,[],{secureProtocol:'TLS_method\0suffix'},{ciphers:'DEFAULT\0suffix'},{ecdhCurve:'auto\0suffix'},{sigalgs:'bad'},{minVersion:'bad'},{maxVersion:'bad'},{secureProtocol:'bad'},{secureProtocol:'SSLv2_method'},{secureProtocol:'SSLv3_client_method'},{secureProtocol:'TLS_method',minVersion:'TLSv1.2'},{secureOptions:1.5},{secureOptions:Infinity},{secureOptions:'4'},{honorCipherOrder:true,secureOptions:'4'},{sessionTimeout:-1},{sessionTimeout:1.5},{sessionTimeout:'2'},{sigalgs:''},{sigalgs:1},{ecdhCurve:1},{ca:1},{cert:1},{key:1},{key:['bad'],passphrase:1},{certificateCompression:'zlib'},{certificateCompression:['bad']},{certificateCompression:['zlib','brotli','zstd','zlib']},{ticketKeys:1},{ticketKeys:Buffer.alloc(47)}];
for(const options of forms)try{const context=tls.createSecureContext(options);console.log(JSON.stringify(['ok',context.context.getMinProto(),context.context.getMaxProto()]));}catch(e){console.log(JSON.stringify([e.name,e.code,e.message,Object.keys(e),e.constructor.name]));}
"#,
    );
}


#[test]
fn native_tls_context_option_getter_and_native_method_hook_order_match_node() {
    compare_node_stream_source(
        r#"
const tls=require('tls'),assert=require('assert'),log=[];const options={};
for(const name of ['honorCipherOrder','minVersion','maxVersion','secureProtocol','secureOptions','allowPartialTrustChain','ca','cert','certificateCompression','ciphers','clientCertEngine','crl','dhparam','ecdhCurve','key','passphrase','pfx','privateKeyIdentifier','privateKeyEngine','sessionIdContext','sessionTimeout','sigalgs','ticketKeys'])Object.defineProperty(options,name,{get(){log.push(name);return undefined;}});
tls.createSecureContext(options);console.log(JSON.stringify(log));
const proto=Object.getPrototypeOf(tls.createSecureContext().context),originals={};for(const name of ['setCiphers','setCipherSuites','addRootCerts','setECDHCurve']){originals[name]=proto[name];proto[name]=function(...args){log.push(name);return originals[name].apply(this,args);};}log.length=0;tls.createSecureContext();console.log(JSON.stringify(log));for(const name of Object.keys(originals))proto[name]=originals[name];
const marker={};assert.throws(()=>tls.createSecureContext({get ca(){throw marker}}),e=>e===marker);console.log('original-getter-error');
"#,
    );
}


#[test]
fn native_tls_context_retains_real_certificate_material_and_view_offsets_match_node() {
    let cert = include_str!("../../../tests/node/test/fixtures/keys/agent1-cert.pem");
    let key = include_str!("../../../tests/node/test/fixtures/keys/agent1-key.pem");
    let source = format!(
        r#"
const assert=require('assert'),tls=require('tls'),OriginalBuffer=Buffer,originalFrom=Buffer.from;
const cert={},key={};
OriginalBuffer.from=()=>{{throw new Error('public Buffer.from used')}};
globalThis.Buffer=()=>{{throw new Error('public Buffer used')}};
const held=tls.createSecureContext({{cert,key}});
const expected=held.context.getCertificate();
assert(OriginalBuffer.isBuffer(expected));
globalThis.Buffer=OriginalBuffer;OriginalBuffer.from=originalFrom;
process.on('exit',()=>{{assert.deepStrictEqual(held.context.getCertificate(),expected);console.log('exit-certificate-retained');}});
function view(text){{const bytes=Buffer.from(text);const prefix=Buffer.from('prefix');const padded=Buffer.concat([prefix,bytes,Buffer.from('suffix')]);return new DataView(padded.buffer,padded.byteOffset+prefix.length,bytes.length);}}
for(const options of [{{cert,key}},{{cert:[cert],key:[key]}},{{cert:view(cert),key:view(key)}},{{ca:cert,cert,key}}]){{const ctx=tls.createSecureContext(options);assert.deepStrictEqual(ctx.context.getCertificate(),expected);console.log(ctx.context.getCertificate().length);}}
const ctx=tls.createSecureContext();assert.strictEqual(ctx.context.getCertificate(),null);ctx.context.setCert(cert);assert.deepStrictEqual(ctx.context.getCertificate(),expected);console.log('retained-native-certificate');
"#,
        serde_json::to_string(cert).unwrap(),
        serde_json::to_string(key).unwrap()
    );
    compare_node_stream_source(&source);
}


#[test]
fn native_tls_context_engine_errors_and_shared_crypto_warning_match_node() {
    compare_node_stream_source(
        r#"
const tls=require('tls'),crypto=require('crypto'),warnings=[];
process.emitWarning=function(...args){warnings.push(args);};
for(const action of [()=>tls.createSecureContext({clientCertEngine:'quench_missing_engine'}),()=>tls.createSecureContext({privateKeyIdentifier:'key',privateKeyEngine:'quench_missing_engine'}),()=>crypto.setEngine('quench_missing_engine'),()=>crypto.setEngine('quench_missing_engine',false),()=>crypto.setEngine(1),()=>crypto.setEngine('quench_missing_engine','1'),()=>tls.createSecureContext({clientCertEngine:1}),()=>tls.createSecureContext({privateKeyIdentifier:1,privateKeyEngine:'x'}),()=>tls.createSecureContext({privateKeyIdentifier:'x',privateKeyEngine:1})])try{action();console.log('ok');}catch(e){console.log(JSON.stringify([e.name,e.code,e.message,Object.keys(e),e.opensslErrorStack]));}
console.log(JSON.stringify(warnings));
"#,
    );
}

#[test]
fn native_tls_context_internal_binding_constructor_snapshot_and_missing_engine_hooks_match_node() {
    const SOURCE: &str = r#"
const assert=require('assert'),warnings=[];process.emitWarning=function(...args){warnings.push(args);};
const {internalBinding}=require('internal/test/binding'),binding=internalBinding('crypto');assert.strictEqual(binding,internalBinding('crypto'));
const Native=binding.SecureContext;let creations=0;
binding.SecureContext=function(){creations++;const context=new Native();context.setClientCertEngine=undefined;context.setEngineKey=undefined;return context;};
const tls=require('tls');
for(const options of [{clientCertEngine:'x'},{privateKeyIdentifier:'key',privateKeyEngine:'x'}])try{tls.createSecureContext(options);}catch(e){console.log(JSON.stringify([e.name,e.code,e.message,Object.keys(e)]));}
binding.SecureContext=function(){throw new Error('late constructor read')};
console.log(tls.createSecureContext().context.getMinProto(),creations);console.log(JSON.stringify(warnings));
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--no-warnings", "--expose-internals", "-e", SOURCE]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code,Some(0),"{}",String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(),"{}",String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(observations[0].stdout,observations[1].stdout);
}

#[test]
fn native_internal_key_filters_and_raw_engine_binding_match_node() {
    const SOURCE: &str = r#"
const {internalBinding}=require('internal/test/binding');const u=internalBinding('util'),c=internalBinding('crypto');
const fs=require('fs'),fc=internalBinding('constants').fs,fd=Object.getOwnPropertyDescriptor(fs,'constants');
console.log(JSON.stringify(['fs-constants',fs.constants===fc,[fd.writable,fd.enumerable,fd.configurable],['UNKNOWN','FILE','DIR','LINK','FIFO','SOCKET','CHAR','BLOCK'].map(k=>[k,Object.getOwnPropertyDescriptor(fc,'UV_DIRENT_'+k)])]));
console.log(JSON.stringify([c.setEngine.name,c.setEngine.length,u.getOwnNonIndexProperties.name,u.getOwnNonIndexProperties.length,u===internalBinding('util'),Object.entries(u.constants)]));
for(const flags of [0,1,65535])console.log(JSON.stringify(['dynamic',flags,c.setEngine('dynamic',flags)]));
const a=[1,2];a.extra=3;Object.defineProperty(a,'hidden',{value:4});a[Symbol('x')]=5;a['01']=6;a['4294967295']=7;
const o={data:1};Object.defineProperties(o,{get:{get(){throw Error('getter invoked')},enumerable:true,configurable:true},set:{set(v){},enumerable:true,configurable:true},both:{get(){throw Error('getter invoked')},set(v){},enumerable:true,configurable:true},fixed:{value:2,enumerable:true}});
for(const object of [a,o,new Uint8Array(3),Object.create(null)])for(const flags of [0,1,2,3,4,8,16,17,31])console.log(JSON.stringify([flags,u.getOwnNonIndexProperties(object,flags).map(k=>typeof k==='symbol'?String(k):k)]));
for(const flags of [0,1,2,3,4,8,16,17,31]){const trace=[];const p=new Proxy({a:1},{ownKeys(){trace.push('keys');return ['a','ghost','0']},getOwnPropertyDescriptor(t,k){trace.push('desc:'+k);return Object.getOwnPropertyDescriptor(t,k)}});console.log(JSON.stringify([flags,u.getOwnNonIndexProperties(p,flags),trace]));}
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--no-warnings", "--expose-internals", "-e", SOURCE]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1"); }
        quench_node_test::case_process::observe_command(command,Duration::from_secs(10)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code,Some(0),"{}",String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(),"{}",String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(String::from_utf8_lossy(&observations[0].stdout),String::from_utf8_lossy(&observations[1].stdout));
}

#[test]
fn native_tls_bundled_roots_preserve_stable_trust_material_cache_metadata_and_intrinsic_hooks() {
    compare_node_stream_source(r#"
const tls=require('tls'),crypto=require('crypto');
const O=Object,A=Array,descriptor=O.getOwnPropertyDescriptor(tls,'rootCertificates'),getter=descriptor.get;
console.log(JSON.stringify([descriptor.enumerable,descriptor.configurable,descriptor.set,getter.name,getter.length,O.hasOwn(getter,'prototype')]));
globalThis.Array=function(){throw new Error('public Array used')};globalThis.Object=function(){throw new Error('public Object used')};
const roots=getter.call({});
console.log(JSON.stringify([A.isArray(roots),O.isFrozen(roots),roots===tls.rootCertificates,Reflect.construct(getter,[])===roots,roots.length,new Set(roots).size]));
globalThis.Array=A;globalThis.Object=O;
console.log(crypto.createHash('sha256').update(JSON.stringify(roots)).digest('hex'));
for(const change of [()=>{'use strict';roots[0]='changed'},()=>{'use strict';tls.rootCertificates=[]},()=>roots.push('changed')])try{change();console.log('unexpected mutation')}catch(e){console.log(e.name)}
"#);
}

#[test]
fn native_tls_cipher_discovery_and_cached_copy_contract_match_node() {
    compare_node_stream_source(r#"
const tls=require('tls');const first=tls.getCiphers();console.log(JSON.stringify(first));
console.log(JSON.stringify([tls.getCiphers.name,tls.getCiphers.length,Object.hasOwn(tls.getCiphers,'prototype'),Object.getOwnPropertyDescriptor(tls,'getCiphers')]));
first.length=0;first.push('guest mutation');const next=tls.getCiphers();console.log(JSON.stringify([first===next,next.length,next.includes('guest mutation'),next.every(x=>x===x.toLowerCase())]));
try{Reflect.construct(tls.getCiphers,[]);}catch(e){console.log(e.name);}
globalThis.Array=function(){throw new Error('public Array used')};console.log(tls.getCiphers().length);
"#);
}

#[test]
fn native_tls_hostname_authorization_values_errors_and_getter_order_match_node() {
    compare_node_stream_source(r#"
const tls=require('tls');
for(const [host,cert] of [
['a.example.com',{subject:{CN:'*.example.com'}}],['a.b.example.com',{subject:{CN:'*.example.com'}}],
['bücher.example.com',{subject:{CN:'xn--bcher-kva.example.com'}}],['a.com',{subject:{CN:'a.com'},subjectaltname:'DNS:b.com'}],
['127.0.0.1',{subjectaltname:'IP Address:127.0.0.1'}],['::1',{subjectaltname:'IP Address:0:0:0:0:0:0:0:1'}],
['a,com',{subjectaltname:'DNS:"a,com"'}],['a.com',{subjectaltname:'DNS:"unterminated'}],
[false,{subject:{CN:['a.com','b.com']}}],['a.com',{}]
])try{const e=tls.checkServerIdentity(host,cert);console.log(e?JSON.stringify([e.name,e.code,e.message,e.reason,e.host,e.cert===cert,Object.keys(e)]):'valid');}catch(e){console.log(JSON.stringify(['throw',e.name,e.code,e.message,Object.keys(e)]));}
const log=[];const host={valueOf(){log.push('host.valueOf');return 'a.com'},toString(){throw new Error('wrong string hint')}};
const cert={get subject(){log.push('subject');return {get CN(){log.push('CN');return 'a.com'}}},get subjectaltname(){log.push('alt');return ''}};
console.log(tls.checkServerIdentity(host,cert),JSON.stringify(log));
console.log(tls.checkServerIdentity.name,tls.checkServerIdentity.length,Object.hasOwn(tls.checkServerIdentity,'prototype'));
const hooks=[];const alt={includes(v){hooks.push(['includes',v]);return false},split(v){hooks.push(['split',v]);return {forEach(cb){hooks.push('forEach');cb('DNS:a.com')}}},toString(){throw new Error('unused alt coercion')}};
console.log(tls.checkServerIdentity('a.com',{subjectaltname:alt}),JSON.stringify(hooks));
console.log(tls.checkServerIdentity('A.COM',{subject:{CN:new String('a.com')}}));
try{tls.checkServerIdentity('a.com',{subject:{CN:123}})}catch(e){console.log('numeric CN',e.name)}
const originalReplace=String.prototype.replace,patterns=[],callbacks=[];
String.prototype.replace=function(pattern,replacer){patterns.push(pattern);if(typeof replacer==='function')callbacks.push(replacer);return originalReplace.call(this,pattern,replacer)};
try{for(let i=0;i<2;i++)tls.checkServerIdentity('A.COM',{subject:{CN:'a.com'}})}finally{String.prototype.replace=originalReplace}
console.log(JSON.stringify([patterns.length,new Set(patterns).size,callbacks.every(c=>c===callbacks[0]),callbacks.map(c=>[c.name,c.length,Object.hasOwn(c,'prototype')])])) ;
"#);
}

#[test]
fn native_net_delivery_gates_preserve_connection_before_bytes_and_write_completion() {
    compare_node_stream_source(r#"
const net=require('net');const log=[];let connected=false,accepted=false,writeAfterConnect=false,readAfterAccept=false;
const server=net.createServer(socket=>{accepted=true;socket.on('data',chunk=>{readAfterAccept=accepted;socket.end(chunk)});});
server.listen(0,'127.0.0.1',()=>{const socket=net.connect(server.address().port,'127.0.0.1');let text='';socket.on('connect',()=>{connected=true});socket.write('queued-before-connect',()=>{writeAfterConnect=connected});socket.on('data',chunk=>text+=chunk);socket.on('close',()=>server.close(()=>console.log(JSON.stringify([text,writeAfterConnect,readAfterAccept]))));});
"#);
}

fn tls_peer_first_line(child: &mut std::process::Child) -> (std::io::BufReader<std::process::ChildStdout>, String) {
    use std::io::{BufRead, BufReader};
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        let mut line = String::new();
        let result = stdout.read_line(&mut line);
        let _ = send.send((stdout, line, result));
    });
    let result = receive.recv_timeout(Duration::from_secs(5));
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    reader.join().unwrap();
    let (stdout, line, result) = result.expect("TLS peer did not announce its port before the startup deadline");
    result.expect("TLS peer stdout failed");
    (stdout, line)
}

#[test]
fn native_tls_client_exchanges_encrypted_bytes_with_independent_node_peer() {
    struct Peer(std::process::Child);
    impl Drop for Peer {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
    let fixtures=std::env::var("QUENCH_NODE_LIBRARY_ROOT").unwrap_or_else(|_|format!("{}/../../tests/node",env!("CARGO_MANIFEST_DIR")));
    let mut peer=Peer(Command::new("node").args(["-e",r#"
const tls=require('tls'),fs=require('fs'),path=require('path');const root=process.env.QUENCH_TLS_PEER_ROOT;
const server=tls.createServer({key:fs.readFileSync(path.join(root,'test/fixtures/keys/agent1-key.pem')),cert:fs.readFileSync(path.join(root,'test/fixtures/keys/agent1-cert.pem'))},socket=>{socket.on('error',()=>{});socket.on('data',chunk=>socket.write(chunk));socket.on('end',()=>socket.end());});
server.on('tlsClientError',()=>{});server.listen(0,'127.0.0.1',()=>console.log(server.address().port));setTimeout(()=>process.exit(0),30000);
"#]).env("QUENCH_TLS_PEER_ROOT",&fixtures).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap());
    let (_, line) = tls_peer_first_line(&mut peer.0);
    let port:u16=line.trim().parse().expect("independent TLS peer did not start");
    let source=format!(r#"
const tls=require('tls'),net=require('net');const socket=tls.connect({{port:{port},host:'127.0.0.1',rejectUnauthorized:false}});let result=[];
socket.once('secureConnect',()=>{{console.log(JSON.stringify([socket instanceof tls.TLSSocket,socket instanceof net.Socket,socket.encrypted,socket.secureConnecting,socket.authorized,socket.authorizationError,socket.getProtocol()]));socket.end(Buffer.from([0,1,2,127,128,255,10,13]));}});
socket.on('data',chunk=>result.push(chunk));socket.on('end',()=>console.log(JSON.stringify(Array.from(Buffer.concat(result)))));
socket.on('error',error=>{{console.log(JSON.stringify(['error',error.name,error.code,error.message]));process.exitCode=1}});
"#);
    let trusted=format!(r#"
const tls=require('tls'),fs=require('fs');const ca=fs.readFileSync(process.env.QUENCH_TLS_PEER_ROOT+'/test/fixtures/keys/ca1-cert.pem');
const socket=tls.connect({{port:{port},host:'127.0.0.1',servername:'agent1',ca}});let text='';
socket.on('secureConnect',()=>{{const cert=socket.getPeerCertificate();console.log(JSON.stringify([socket.authorized,socket.authorizationError,cert.subject.CN,cert.issuer.CN,Buffer.isBuffer(cert.raw),cert.bits,cert.exponent]));socket.end('trusted-peer');}});
socket.on('data',chunk=>text+=chunk);socket.on('end',()=>console.log(text));socket.on('error',e=>{{console.log(JSON.stringify([e.name,e.code,e.message]));process.exitCode=1}});
"#);
    let rejected=format!(r#"
const tls=require('tls');const socket=tls.connect({{port:{port},host:'127.0.0.1'}});
socket.on('secureConnect',()=>{{console.log('unexpected secure');socket.end()}});socket.on('error',e=>console.log(JSON.stringify([e.name,e.code,e.message])));socket.on('close',()=>console.log('closed'));
"#);
    let mismatch=format!(r#"
const tls=require('tls'),fs=require('fs');const socket=tls.connect({{port:{port},host:'127.0.0.1',servername:'wrong.invalid',ca:fs.readFileSync(process.env.QUENCH_TLS_PEER_ROOT+'/test/fixtures/keys/ca1-cert.pem')}});
socket.on('secureConnect',()=>{{console.log('unexpected secure');socket.end()}});socket.on('error',e=>console.log(JSON.stringify([e.name,e.code,e.message,e.reason,e.host,e.cert?.subject.CN])));socket.on('close',()=>console.log('closed'));
"#);
    let direct=format!(r#"
const tls=require('tls'),net=require('net');const socket=new tls.TLSSocket(undefined,{{rejectUnauthorized:true,checkServerIdentity:()=>{{throw new Error('direct socket must not authorize')}}}});let result=[];
console.log(JSON.stringify([socket.getProtocol(),socket.getPeerCertificate(),socket.isSessionReused()]));
socket.once('secure',()=>{{console.log(JSON.stringify([socket instanceof tls.TLSSocket,socket instanceof net.Socket,socket.encrypted,socket.secureConnecting,socket.authorized,socket.authorizationError,socket.getProtocol()]));socket.end(Buffer.from([0,1,2,127,128,255,10,13]));}});
socket.on('secureConnect',()=>{{throw new Error('direct socket must emit secure')}});socket.on('data',chunk=>result.push(chunk));socket.on('end',()=>console.log(JSON.stringify(Array.from(Buffer.concat(result)))));
socket.on('error',error=>{{console.log(JSON.stringify(['error',error.name,error.code,error.message]));process.exitCode=1}});socket.connect({{port:{port},host:'127.0.0.1'}});socket._start();
"#);
    let direct_write = direct
        .replace("socket.end(Buffer.from([0,1,2,127,128,255,10,13]));", "")
        .replace("let result=[];", "let result=[];const initialHandle=socket.ssl;")
        .replace("socket.once('secure',()=>{", "socket.once('secure',()=>{console.log(JSON.stringify([socket.ssl===initialHandle,socket.ssl===socket._handle,socket.ssl.verifyError()?.code]));")
        .replace("socket.connect(", "socket.on('connect',()=>socket.end(Buffer.from([0,1,2,127,128,255,10,13])));socket.connect(")
        .replace("socket._start();", "");
    for source in [source,trusted,rejected,mismatch,direct,direct_write] {
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.args(["-e",&source]).env("QUENCH_TLS_PEER_ROOT",&fixtures);
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1").env("QUENCH_NODE_LIBRARY_ROOT",&fixtures);}
        quench_node_test::case_process::observe_command(command,Duration::from_secs(10)).unwrap()
    });
    assert!(!observations[0].timed_out,"Node TLS oracle timed out");
    assert!(!observations[1].timed_out,"native TLS client timed out: {}",String::from_utf8_lossy(&observations[1].stderr));
    assert_eq!(observations[0].exit_code,Some(0),"Node TLS oracle: {}",String::from_utf8_lossy(&observations[0].stderr));
    assert_eq!(observations[1].exit_code,Some(0),"native TLS client: {} stdout: {} case: {source}",String::from_utf8_lossy(&observations[1].stderr),String::from_utf8_lossy(&observations[1].stdout));
    assert_eq!(String::from_utf8_lossy(&observations[1].stdout),String::from_utf8_lossy(&observations[0].stdout),"case: {source}");
    }
}

#[test]
fn native_tls_server_accepts_encrypted_bytes_from_independent_node_client() {
    use std::io::Read;
    struct Peer(std::process::Child);
    impl Drop for Peer {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
    let fixtures=std::env::var("QUENCH_NODE_LIBRARY_ROOT").unwrap_or_else(|_|format!("{}/../../tests/node",env!("CARGO_MANIFEST_DIR")));
    let server_source=r#"
const tls=require('tls'),net=require('net'),fs=require('fs'),path=require('path');const root=process.env.QUENCH_TLS_PEER_ROOT;
const server=tls.createServer({key:fs.readFileSync(path.join(root,'test/fixtures/keys/agent1-key.pem')),cert:fs.readFileSync(path.join(root,'test/fixtures/keys/agent1-cert.pem'))},socket=>{console.log(JSON.stringify([socket instanceof tls.TLSSocket,socket instanceof net.Socket,socket.encrypted,socket.secureConnecting,socket.getProtocol()]));socket.on('data',chunk=>socket.end(chunk));socket.on('close',()=>server.close());socket.on('error',()=>{});});
server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
"#;
    let direct_server_source=r#"
const tls=require('tls'),net=require('net'),fs=require('fs'),path=require('path');const root=process.env.QUENCH_TLS_PEER_ROOT;
const context=tls.createSecureContext({key:fs.readFileSync(path.join(root,'test/fixtures/keys/agent1-key.pem')),cert:fs.readFileSync(path.join(root,'test/fixtures/keys/agent1-cert.pem'))});
const server=net.createServer(raw=>{const socket=new tls.TLSSocket(raw,{isServer:true,secureContext:context});socket.on('secure',()=>{console.log(JSON.stringify([socket instanceof tls.TLSSocket,socket instanceof net.Socket,socket.encrypted,socket.secureConnecting,socket.getProtocol()]));socket.on('data',chunk=>socket.end(chunk));socket.on('close',()=>server.close());});socket.on('error',()=>{});});
server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
"#;
    let delayed_server_source = direct_server_source
        .replace("raw=>{const socket=", "raw=>{setTimeout(()=>{console.log(JSON.stringify([raw.readableLength>0,raw.bytesRead>0,raw.readableFlowing==null]));const socket=")
        .replace("socket.on('error',()=>{});});", "socket.on('error',()=>{});},100);});");
    let abrupt_server_source = direct_server_source.replace("chunk=>socket.end(chunk)", "chunk=>socket.destroy()");
    // Keep the abrupt-close client open until the server destroys its socket.
    // Racing both peers' shutdown can make even Node's client alternate between
    // clean EOF and ECONNRESET. Compare full outcomes on a directed exchange.
    for (server_source, half_close) in [(server_source,true),(direct_server_source,true),(delayed_server_source.as_str(),true),(abrupt_server_source.as_str(),false)] {
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.args(["-e",server_source]).env("QUENCH_TLS_PEER_ROOT",&fixtures).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1").env("QUENCH_NODE_LIBRARY_ROOT",&fixtures);}
        let mut peer=Peer(command.spawn().unwrap());let (mut stdout,line)=tls_peer_first_line(&mut peer.0);
        let port:u16=line.trim().parse().expect("TLS server did not start");
        let send=if half_close {"end"} else {"write"};
        let source=format!(r#"
const tls=require('tls'),fs=require('fs');const socket=tls.connect({{port:{port},host:'127.0.0.1',servername:'agent1',ca:fs.readFileSync(process.env.QUENCH_TLS_PEER_ROOT+'/test/fixtures/keys/ca1-cert.pem')}});let chunks=[];
socket.on('secureConnect',()=>socket.{send}(Buffer.from([0,22,23,127,254,255])));socket.on('data',chunk=>chunks.push(chunk));socket.on('end',()=>console.log(JSON.stringify([socket.authorized,Array.from(Buffer.concat(chunks))])));socket.on('error',e=>{{console.log(e.code,e.message);process.exitCode=1}});
"#);
        let mut client=Command::new("node");client.args(["-e",&source]).env("QUENCH_TLS_PEER_ROOT",&fixtures);
        let result=quench_node_test::case_process::observe_command(client,Duration::from_secs(10)).unwrap();
        assert!(!result.timed_out,"external client timed out");assert_eq!(result.signal,None,"external client crashed");
        assert_eq!(result.exit_code,Some(0),"engine {executable}; server {server_source}; client stdout: {}; stderr: {}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
        let deadline=std::time::Instant::now()+Duration::from_secs(5);
        let status=loop{if let Some(status)=peer.0.try_wait().unwrap(){break status;}if std::time::Instant::now()>=deadline{let _=peer.0.kill();let _=peer.0.wait();panic!("TLS server did not close after peer EOF");}std::thread::sleep(Duration::from_millis(10));};
        let mut rest=String::new();stdout.read_to_string(&mut rest).unwrap();
        assert!(status.success(),"TLS server failed");
        (result.exit_code,result.signal,result.stdout,result.stderr,rest)
    });
    assert_eq!(observations[1],observations[0]);
    }
}

#[test]
fn native_net_resource_visibility_follows_original_connection_delivery_and_close() {
    compare_node_stream_source(r#"
const net=require('net');let peer,client;const count=t=>process.getActiveResourcesInfo().filter(x=>x===t).length;
const server=net.createServer(socket=>{peer=socket;});server.listen(0,'127.0.0.1',()=>{client=net.connect(server.address().port,'127.0.0.1',()=>setImmediate(()=>{
console.log(JSON.stringify([count('TCPSocketWrap'),count('TCPServerWrap')]));client.unref();peer.unref();server.unref();console.log(JSON.stringify([count('TCPSocketWrap'),count('TCPServerWrap')]));client.ref();peer.ref();server.ref();console.log(JSON.stringify([count('TCPSocketWrap'),count('TCPServerWrap')]));client.destroy();peer.end();console.log(count('TCPServerWrap'));server.close(()=>console.log(JSON.stringify([count('TCPSocketWrap'),count('TCPServerWrap')])));
}));});
"#);
}

#[test]
fn native_net_unconsumed_bytes_remain_buffered_and_count_before_data_listeners() {
    compare_node_stream_source(r#"
const net=require('net');let client;
const server=net.createServer(socket=>{setTimeout(()=>{
console.log(JSON.stringify([socket.readableLength,socket.bytesRead,socket.readableFlowing==null]));
const data=socket.read();console.log(JSON.stringify(Array.from(data)));socket.end(data);socket.on('close',()=>server.close());
},100);});
server.listen(0,'127.0.0.1',()=>{client=net.connect(server.address().port,'127.0.0.1',()=>client.end(Buffer.from([0,127,128,255])));client.on('data',data=>console.log(JSON.stringify(Array.from(data))));});
"#);
}

#[test]
fn native_net_accepted_resources_are_visible_inside_original_connection_callback() {
    compare_node_stream_source(r#"
const net=require('net');let client;
const server=net.createServer(socket=>{console.log(process.getActiveResourcesInfo().filter(x=>x==='TCPSocketWrap').length);socket.end();socket.on('close',()=>server.close());});
server.listen(0,'127.0.0.1',()=>{client=net.connect(server.address().port,'127.0.0.1');client.resume();});
"#);
}

#[test]
fn net_socket_end_listener_and_write_after_fin_match_node() {
    compare_node_stream_source(r#"
const net=require('net'); const out=[];
const reads=[];
const options={get allowHalfOpen(){reads.push('allowHalfOpen');return true;},get noDelay(){reads.push('noDelay');return true;},get extra(){reads.push('extra');return 7;}};
const copied=new net.Socket(options);out.push(['options',reads,copied.allowHalfOpen]);copied.destroy();
for(const half of [false,true]){
 const s=new net.Socket({allowHalfOpen:half}); const original=s.write;
 out.push([half,s.listenerCount('end'),s.listeners('end').map(f=>[f.name,f.length])]);
 const listener=s.listeners('end')[0];
 for(const receiver of [null,undefined]){try{listener.call(receiver);out.push(['receiver','accepted']);}catch(e){out.push(['receiver',e.name]);}}
 s.emit('end'); out.push([half,s.write===original,s.write===original?null:s.write.name,s.write.length]);
 if(!half){
  for(const receiver of [null,undefined]){try{s.write.call(receiver,'x');out.push(['write receiver','accepted']);}catch(e){out.push(['write receiver',e.name]);}}
  const Duplex=require('stream').Duplex,originalBase=Duplex.prototype.write,marker={};
  Duplex.prototype.write=function(chunk,encoding,cb){out.push(['delegate',this===s,chunk,encoding,cb]);return marker;};
  out.push(['delegated return',s.write('x','ascii',null)===marker]);Duplex.prototype.write=originalBase;
 }
 s.destroy();
}
const server=net.createServer(s=>s.end()).listen(0,()=>{
 let callbackError;
 const client=net.connect(server.address().port,()=>client.end());
 client.on('error',e=>{out.push(['error',e.code,e.message,callbackError===e]);});
 client.on('end',()=>{out.push(['end',client.writableEnded]);out.push(['return',client.write('x',e=>{callbackError=e;out.push(['callback',e.code,e.message]);})]);out.push(['sync']);});
 client.on('close',()=>server.close(()=>console.log(JSON.stringify(out))));
});
"#);
}

#[test]
fn net_server_uses_original_event_emitter_protocol() {
    compare_node_stream_source(r#"
const net=require('net'),{EventEmitter}=require('events');const output=[];
const server=net.createServer();
output.push(['inherit',Object.getPrototypeOf(net.Server)===EventEmitter,Object.getPrototypeOf(net.Server.prototype)===EventEmitter.prototype,server instanceof EventEmitter]);
output.push(['empty',server.emit('unknown'),server.eventNames(),server.getMaxListeners()]);
const original={};try{server.emit('error',original);}catch(error){output.push(['error identity',error===original,error.code]);}
const events=[];server.on('newListener',(type,callback)=>events.push(['add',type,callback.name]));
function callback(){events.push(['callback',this===server]);}
server.once('value',callback);server.prependListener('value',function first(){events.push(['first',this===server]);});
output.push(['raw',server.listeners('value')[1]===callback,server.rawListeners('value')[1].listener===callback]);
output.push(['emit',server.emit('value'),server.emit('value')]);server.removeAllListeners('value');
output.push(['events',events]);
console.log(JSON.stringify(output));
"#);
}

#[test]
fn net_resource_counts_exclude_undelivered_accepted_wrappers() {
    compare_node_stream_source(r#"
const net=require('net'),assert=require('assert');const clients=[],accepted=[];let completed=0;
const server=net.createServer(socket=>accepted.push(socket)).listen(0,next);
function next(){const client=net.connect(server.address().port,()=>{if(++completed===8){const count=process.getActiveResourcesInfo().filter(x=>x==='TCPSocketWrap').length;assert.strictEqual(count,clients.length+accepted.length);console.log('resource-count-matches-delivered-wrappers');for(const socket of clients)socket.destroy();for(const socket of accepted)socket.end();server.close();}else next();});clients.push(client);}
"#);
}

#[test]
fn string_repeat_node_limit_errors_and_lazy_lengths_match_node() {
    compare_node_stream_source(r#"
const assert=require('assert');const limit=require('buffer').constants.MAX_STRING_LENGTH;const out=[];
for(const count of [-0.5,-0,NaN,undefined,0,1,2]){out.push([String(count),'xy'.repeat(count)]);}
assert.strictEqual('x'.repeat(limit).length,limit);
assert.strictEqual('x'.repeat(70000000).repeat(2).length,140000000);
for(const input of ['x','xy','日']){const count=Math.floor(limit/input.length)+1;try{input.repeat(count);out.push('accepted');}catch(e){out.push([e.name,e.message]);}}
for(const count of [-1,-1.5,Infinity,-Infinity]){try{'x'.repeat(count);out.push('accepted');}catch(e){out.push([e.name,e.message]);}}
const order=[];const receiver={toString(){order.push('receiver');return 'xy';}};const count={valueOf(){order.push('count');return -0.5;}};out.push([String.prototype.repeat.call(receiver,count),order]);
console.log(JSON.stringify(out));
"#);
}

#[test]
fn net_create_server_preserves_options_connection_listener_and_arity() {
    compare_node_stream_source(r#"
const net=require('net');const output=[];function listener(){}
for(const args of [[],[listener],[{},listener],[null,listener],[undefined,listener]]){const server=net.createServer(...args);output.push([server instanceof net.Server,server.listeners('connection').length,server.listeners('connection')[0]===listener]);}
output.push([net.createServer.name,net.createServer.length]);console.log(JSON.stringify(output));
"#);
}

#[test]
fn net_socket_option_normalization_cache_and_metadata_match_node() {
    compare_node_stream_source(r#"
const net=require('net');const out=[];
const socket=new net.Socket({noDelay:false,keepAlive:true,keepAliveInitialDelay:1000});
socket._handle={setNoDelay(...args){out.push(['nodelay',args,this===socket._handle]);return 0;},setKeepAlive(...args){out.push(['keepalive',args,this===socket._handle]);return 0;}};
for(const enable of [undefined,true,true,false,false])out.push(['returnNoDelay',socket.setNoDelay(enable)===socket]);
for(const args of [[true,1000],[true,2000],[true,3000],[false,4000],[false,5000],[true,0],[{enable:true,initialDelay:1234,interval:3456,count:9}]])out.push(['returnKeepAlive',socket.setKeepAlive(...args)===socket]);
out.push(['metadata',net.Socket.prototype.setNoDelay.name,net.Socket.prototype.setNoDelay.length,net.Socket.prototype.setKeepAlive.name,net.Socket.prototype.setKeepAlive.length]);
socket._handle=null;socket.destroy();console.log(JSON.stringify(out));
"#);
}

#[test]
fn original_socket_option_exception_preserves_identity_and_close_progress() {
    compare_node_stream_source(r#"
const net=require('net'),assert=require('assert');const marker={};let client;const out=[];
const server=net.createServer(socket=>socket.end()).listen(0,()=>{
 client=net.connect({port:server.address().port,noDelay:true});
 client._handle.setNoDelay=()=>{out.push('option');throw marker;};
 client.on('connect',()=>assert.fail('connect after original option exception'));
});
process.once('uncaughtException',(error,origin)=>{out.push([error===marker,origin]);client.destroy();server.close(()=>console.log(JSON.stringify(out)));});
"#);
}

#[test]
fn net_server_admission_counts_and_drop_addresses_match_node() {
    compare_node_stream_source(r#"
'use strict';
const net=require('net'),assert=require('assert');const output=[];
const server=net.createServer(socket=>{
 output.push(['accepted',server._connections,socket.server===server,socket._server===server]);
 socket.on('data',data=>socket.write(data));
});
server.maxConnections=1;
server.on('drop',info=>{
 output.push(['drop',server._connections,Object.getPrototypeOf(info)===null,info.localPort===server.address().port,typeof info.remotePort,info.localFamily,info.remoteFamily]);
});
server.listen(0,()=>{
 const first=net.connect(server.address().port,()=>first.write('one'));
 first.once('data',()=>{
  const second=net.connect(server.address().port);
  second.once('close',()=>{
   output.push(['beforeDestroy',server._connections]);first.destroy();
   server.getConnections((error,count)=>{output.push(['count',error===null,count]);server.close(()=>console.log(JSON.stringify(output)));});
  });
 });
});
"#);
}

#[test]
fn net_server_original_admission_hooks_errors_and_count_timing_match_node() {
    compare_node_stream_source(r#"
'use strict';
const net=require('net');const output=[];
const server=net.createServer().listen(0,()=>{
 const original=server._handle.onconnection;
 output.push(['metadata',original.name,original.length,Object.hasOwn(original,'prototype')]);
 server.maxConnections={valueOf(){output.push('limit');return 0;}};
 const handle={
  get getsockname(){output.push('get-local');return function(info){output.push(['local',this===handle,Object.getPrototypeOf(info)===null]);info.address='local';info.port=12;info.family='IPv4';};},
  get getpeername(){output.push('get-peer');return function(info){output.push(['peer',this===handle,Object.getPrototypeOf(info)===null]);info.address='remote';info.port=34;info.family='IPv6';};},
  get close(){output.push('get-close');return function(){output.push(['close',this===handle]);};},
 };
 server.on('drop',info=>output.push(['drop',Object.getPrototypeOf(info)===null,info]));
 original.call(server._handle,0,handle);
 server.once('error',error=>output.push(['error',error.name,error.code,error.errno,error.syscall,error.message]));
 original.call(server._handle,-22,undefined);
 let synchronous=true;
 const returned=server.getConnections((error,count)=>{
  output.push(['count',error===null,count,synchronous]);
  server.close(()=>console.log(JSON.stringify(output)));
 });
 output.push(['return',returned===server,server.getConnections.name,server.getConnections.length]);
 synchronous=false;
});
"#);
}

#[test]
fn net_raw_handle_close_preserves_mutable_callback_and_first_request() {
    compare_node_stream_source(r#"
'use strict';
const net=require('net');const output=[];
const server=net.createServer(socket=>socket.on('close',()=>server.close(()=>console.log(JSON.stringify(output))))).listen(0,()=>{
 const client=net.connect(server.address().port,()=>setImmediate(()=>{
  const handle=client._handle;
  handle.close(function(){output.push(['first',this===handle]);});
  const key=Object.getOwnPropertySymbols(handle).find(key=>key.description==='handle_onclose');
  output.push(['symbol',!!key]);
  handle[key]=function(){output.push(['replacement',this===handle]);};
  handle.close(()=>output.push('second'));
 }));
});
"#);
}

#[test]
fn net_default_listener_and_native_endpoint_addresses_match_node() {
    compare_node_stream_source(r#"
const net=require('net');const out=[];
const server=net.createServer(socket=>socket.end()).listen(0,()=>{
 const address=server.address();out.push(['server',address.address,address.family]);
 const client=net.connect(address.port,()=>{
  const local={},peer={};const handle=client._handle;
  out.push(['status',handle.getsockname(local),handle.getpeername(peer)]);
  out.push(['endpoints',local.address,local.family,peer.address,peer.family,peer.port===address.port]);
 });
 client.on('close',()=>server.close(()=>console.log(JSON.stringify(out))));
});
"#);
}

#[test]
fn native_stat_exact_bigint_metadata_matches_node_across_sync_callback_and_promises() {
    compare_node_stream_source(r#"
'use strict';
const fs=require('fs'),path=require('path'),os=require('os'),assert=require('assert');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-stat-exact-')),file=path.join(dir,'file'),link=path.join(dir,'link');
fs.writeFileSync(file,'abc');fs.symlinkSync(file,link);fs.utimesSync(file,1700000000.999999,1700000001.123456);
const fd=fs.openSync(file,'r'),out=[];
function check(label,big,num,exact){
 for(const key of ['dev','ino','mode','nlink','uid','gid','rdev','size','blksize','blocks']){assert.strictEqual(typeof big[key],'bigint');if(Number.isSafeInteger(num[key]))assert.strictEqual(big[key],BigInt(num[key]));}
 for(const key of ['atime','mtime','ctime','birthtime']){assert.strictEqual(big[key+'Ns']/1000000n,big[key+'Ms']);assert(Math.abs(num[key+'Ms']-Number(big[key+'Ns']/1000000n))<1.001);}
 assert.strictEqual(big.isFile(),num.isFile());assert.strictEqual(big.isSymbolicLink(),num.isSymbolicLink());
 out.push([label,big.isFile(),big.isSymbolicLink(),exact?big.atimeNs.toString():'link',exact?big.mtimeNs.toString():'link']);
}
(async()=>{
 check('statSync',fs.statSync(file,{bigint:true}),fs.statSync(file),true);
 check('lstatSync',fs.lstatSync(link,{bigint:true}),fs.lstatSync(link),false);
 check('fstatSync',fs.fstatSync(fd,{bigint:true}),fs.fstatSync(fd),true);
 for(const [name,arg] of [['stat',file],['lstat',link],['fstat',fd]]){
  const invoke=options=>new Promise((resolve,reject)=>fs[name](arg,options,(error,stats)=>error?reject(error):resolve(stats)));
  check(name,await invoke({bigint:true}),await invoke({}),name!=='lstat');
 }
 check('promiseStat',await fs.promises.stat(file,{bigint:true}),await fs.promises.stat(file),true);
 check('promiseLstat',await fs.promises.lstat(link,{bigint:true}),await fs.promises.lstat(link),false);
 const handle=await fs.promises.open(file,'r');check('handleStat',await handle.stat({bigint:true}),await handle.stat(),true);await handle.close();
 fs.closeSync(fd);fs.rmSync(dir,{recursive:true,force:true});console.log(JSON.stringify(out));
})().catch(error=>{console.error(error);process.exitCode=1;});
"#);
}

#[test]
fn native_socket_paused_accept_defers_actual_reads_until_resume() {
    compare_node_stream_source(r#"
'use strict';
const net=require('net'),assert=require('assert');const out=[];
const server=net.createServer({pauseOnConnect:true},socket=>{
 socket.on('data',data=>{out.push(['data',data.toString()]);socket.end();});
 setTimeout(()=>{
  out.push(['paused',socket.bytesRead,socket.readableFlowing,socket.isPaused()]);assert.strictEqual(socket.bytesRead,0);socket.resume();
 },20);
}).listen(0,()=>{
 const client=net.connect(server.address().port,()=>client.write('payload'));
 client.on('close',()=>server.close(()=>console.log(JSON.stringify(out))));
});
"#);
}

#[test]
fn stream_resume_preserves_readable_listener_buffer_ownership() {
    compare_node_stream_source(r#"
'use strict';
const {Readable}=require('node:stream');
const out=[];
const stream=new Readable({read(){}});
stream.on('readable',()=>{});
stream.on('data',chunk=>out.push(['data',chunk.toString()]));
stream.resume();
stream.push('retained');
process.nextTick(()=>{
 out.push(['state',stream.readableFlowing,stream.isPaused(),stream.readableLength]);
 out.push(['read',stream.read()?.toString()]);
 console.log(JSON.stringify(out));
});
"#);
}

#[test]
fn async_process_exit_preserves_requested_status_and_stops_rejection_dispatch() {
    compare_node_stream_source(r#"
'use strict';
const {spawnSync}=require('node:child_process');
const output=[];
for(const operation of ['exit','reallyExit'])for(const code of [0,7]) {
 const source=`process.on('exit',code=>console.log('exit',code));process.on('unhandledRejection',()=>console.log('unexpected rejection'));Promise.resolve().then(async()=>{process.nextTick(()=>console.log('unexpected tick'));process.${operation}(${code});});`;
 const result=spawnSync(process.execPath,['-e',source],{encoding:'utf8'});
 output.push([operation,code,result.status,result.signal,result.stdout,result.stderr]);
}
console.log(JSON.stringify(output));
"#);
}

#[test]
fn stream_explicit_flowing_false_prevents_automatic_data_resume() {
    compare_node_stream_source(r#"
'use strict';
const {Readable}=require('stream');const out=[];let reads=0;
const stream=new Readable({read(){reads++;this.push('x');this.push(null);}});
stream.readableFlowing=false;stream.on('data',data=>out.push(['data',data.toString()]));
out.push(['initial',stream.isPaused(),stream.readableFlowing,reads]);
process.nextTick(()=>{out.push(['tick',stream.isPaused(),stream.readableFlowing,reads]);stream.resume();});
stream.on('end',()=>{out.push(['end',reads]);console.log(JSON.stringify(out));});
"#);
}

#[test]
fn native_paused_connected_socket_keeps_reference_bit_without_active_loop_work() {
    compare_node_stream_source(r#"
'use strict';
const net=require('net'),assert=require('assert');const output=[];
const server=net.createServer(socket=>socket.unref()).listen(0,()=>{
 const client=net.connect(server.address().port,()=>output.push(['connected',client._handle.hasRef(),client.isPaused()]));client.pause();
 setTimeout(()=>assert.fail('idle referenced socket kept event loop alive'),100).unref();
}).unref();
process.on('exit',()=>console.log(JSON.stringify(output)));
"#);
}

#[test]
fn tls_wrapped_parent_and_tls_handle_share_original_reading_view() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/node/test/fixtures/keys/");
    let source = r#"
'use strict';
const net=require('net'),tls=require('tls'),fs=require('fs');const output=[];
const root=__FIXTURE_KEYS__;
const server=tls.createServer({key:fs.readFileSync(root+'agent1-key.pem'),cert:fs.readFileSync(root+'agent1-cert.pem')}).listen(0,()=>{
 const plain=net.connect(server.address().port,()=>{
  const secure=tls.connect({socket:plain,rejectUnauthorized:false},()=>{
   const parent=plain._handle,handle=secure._handle,descriptor=Object.getOwnPropertyDescriptor(parent,'reading');
   output.push([typeof descriptor.get,typeof descriptor.set,parent.reading===handle.reading]);
   handle.reading=false;output.push(parent.reading);parent.reading=true;output.push(handle.reading);
   secure.destroy();server.close(()=>console.log(JSON.stringify(output)));
  });
 });
});
"#.replace("__FIXTURE_KEYS__", &serde_json::to_string(&format!("{}/", fixtures.display())).unwrap());
    compare_node_stream_source(&source);
}

#[test]
fn native_socket_endpoint_getters_follow_original_handle_and_cache_like_node() {
    compare_node_stream_source(r#"
'use strict';
const net=require('net'),assert=require('assert');const out=[];
for(const name of ['remoteAddress','remoteFamily','remotePort','localAddress','localFamily','localPort']){
 const d=Object.getOwnPropertyDescriptor(net.Socket.prototype,name);out.push([name,d.enumerable,d.configurable,d.get.name,d.get.length,d.set===undefined,Object.hasOwn(d.get,'prototype')]);
}
const socket=new net.Socket(), calls=[];let fail=true;
const handle={get getpeername(){calls.push('get-peer');return function(o){calls.push(['peer',this===handle,Object.getPrototypeOf(o)===Object.prototype]);o.address='peer';o.family='IPv6';o.port=42;return fail?1:0;};},get getsockname(){calls.push('get-local');return function(o){calls.push(['local',this===handle]);o.address='local';o.family='IPv4';o.port=7;return 1;};}};
socket._handle=handle;socket.connecting=true;out.push(['connecting',socket.remoteAddress]);socket.connecting=false;
out.push(['failed',socket.remoteAddress,!!socket._peername]);fail=false;out.push(['success',socket.remotePort,socket.remoteFamily]);socket._handle=null;out.push(['closed',socket.remoteAddress,socket.localAddress]);
socket._handle=handle;out.push(['local',socket.localAddress,socket.localPort]);socket._handle=null;out.push(['local-after-close',socket.localAddress]);socket._getpeername=()=>({address:'override'});out.push(['override',socket.remoteAddress]);out.push(calls);console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_timer_callbacks_and_resource_projection_match_node() {
    compare_node_stream_source(r#"
'use strict';
const assert=require('assert'),out=[];
for(const value of [null,undefined,0,false,{},'none']){out.push(['clear',String(value),clearTimeout(value),clearInterval(value),clearImmediate(value)]);}
const dormant=setTimeout(()=>assert.fail('dormant timer fired'),10000).unref();
const held=setTimeout(()=>assert.fail('held timer fired'),10000);
const dormantImmediate=setImmediate(()=>{}).unref();
out.push(['resources',process.getActiveResourcesInfo().filter(v=>v==='Timeout').length,process.getActiveResourcesInfo().filter(v=>v==='Immediate').length]);clearTimeout(held);clearTimeout(dormant);clearImmediate(dormantImmediate);
let left=3;const rows=[];
function record(kind,handle,args){assert.strictEqual(this,handle);rows.push([kind,this===handle,args]);if(--left===0){rows.sort((a,b)=>a[0].localeCompare(b[0]));out.push(rows);console.log(JSON.stringify(out));}}
const immediate=setImmediate(function(...args){record.call(this,'immediate',immediate,args)},'i',2);
const timeout=setTimeout(function(...args){const before=process.getActiveResourcesInfo().filter(v=>v==='Timeout').length;clearTimeout(timeout);assert.strictEqual(process.getActiveResourcesInfo().filter(v=>v==='Timeout').length,before-1);record.call(this,'timeout',timeout,args)},1,'t',3);
const interval=setInterval(function(...args){clearInterval(interval);record.call(this,'interval',interval,args)},1,'r',4);
"#);
}

#[test]
fn global_crypto_is_installed_before_commonjs_entry_and_preloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = r#"const descriptor=Object.getOwnPropertyDescriptor(globalThis,'crypto');
console.log(JSON.stringify([typeof globalThis.crypto,descriptor.enumerable,descriptor.configurable,typeof descriptor.get,globalThis.crypto===require('crypto').webcrypto]));"#;
    std::fs::write(directory.path().join("preload.cjs"), source).unwrap();
    std::fs::write(directory.path().join("entry.cjs"), source).unwrap();
    for arguments in [vec!["entry.cjs"], vec!["--require", "./preload.cjs", "entry.cjs"]] {
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.current_dir(directory.path()).args(&arguments);
            if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap()
        });
        for observation in &observations {
            assert!(!observation.timed_out);
            assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
            assert!(observation.stderr.is_empty());
        }
        assert_eq!(observations[0].stdout, observations[1].stdout);
    }
}

#[test]
fn native_performance_queries_preserve_identity_order_coercion_and_errors() {
    compare_node_stream_source(r#"
'use strict';
const {performance:p}=require('perf_hooks');const out=[],log=[];
p.clearMarks();p.clearMeasures();
const a=p.mark('same',{startTime:30}),b=p.mark('same',{startTime:10}),c=p.mark('other',{startTime:20}),m=p.measure('same',{start:5,end:6});
function rows(entries){return entries.map(e=>[e.name,e.entryType,e.startTime,e.duration,e===a,e===b,e===c,e===m]);}
out.push(rows(p.getEntriesByName({toString(){log.push('name');return 'same'}},{toString(){log.push('type');return 'mark'}})));out.push(log);
out.push(rows(p.getEntriesByName('same')));out.push(rows(p.getEntriesByType('mark')));out.push(p.getEntriesByType('unknown').length);
const snapshot=p.getEntriesByName('same','mark');snapshot.length=0;out.push(p.getEntriesByName('same','mark').length);
const surrogate=p.mark('\ud800',{startTime:2});out.push(p.getEntriesByName('\ud800')[0]===surrogate,p.getEntriesByName('\ud801').length);
for(const fn of [()=>p.getEntriesByName(),()=>p.getEntriesByType(),()=>p.getEntriesByName(Symbol('x')),()=>p.getEntriesByType(Symbol('x')),()=>p.getEntries.call({}),()=>p.getEntriesByName.call({},'same')]){try{fn();out.push('accepted')}catch(e){out.push([e.name,e.code||null,e.message]);}}
const brand=Reflect.ownKeys(p).find(key=>typeof key==='symbol'&&key.description==='performance');
out.push(p.getEntries.call(new Proxy(p,{})).length,p.getEntries.call({[brand]:false}).length);
try{p.getEntries.call(Object.create(p))}catch(e){out.push([e.name,e.code,e.message]);}
p.clearMarks();p.clearMeasures();console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_timer_disposal_metadata_and_destroyed_phases_match_node() {
    compare_node_stream_source(r#"
'use strict';
const assert=require('assert'),out=[],rows=[];
for(const [kind,handle] of [['Timeout',setTimeout(()=>assert.fail('disposed timeout fired'),10000)],['Interval',setInterval(()=>assert.fail('disposed interval fired'),10000)],['Immediate',setImmediate(()=>assert.fail('disposed immediate fired'))]]){
 const proto=Object.getPrototypeOf(handle);
 for(const key of ['close',Symbol.dispose]){const d=Object.getOwnPropertyDescriptor(proto,key);out.push([kind,String(key),d?[d.value.name,d.value.length,d.writable,d.enumerable,d.configurable]:null]);}
 out.push([kind,'live',handle._destroyed,handle.hasRef()]);
 out.push([kind,'dispose',handle[Symbol.dispose](),handle._destroyed,handle.hasRef()]);
 if(kind!=='Immediate')out.push([kind,'close',handle.close()===handle,handle._destroyed,handle.hasRef()]);
 handle[Symbol.dispose]();
}
const timeout=setTimeout(function(){rows.push(['timeout','during',this===timeout,this._destroyed,this.hasRef()]);process.nextTick(()=>rows.push(['timeout','after',timeout._destroyed,timeout.hasRef()]));},1);
const interval=setInterval(function(){rows.push(['interval','during',this===interval,this._destroyed,this.hasRef()]);assert.strictEqual(this.close(),this);rows.push(['interval','after',this._destroyed,this.hasRef()]);},1);
const immediate=setImmediate(function(){rows.push(['immediate','during',this===immediate,this._destroyed,this.hasRef()]);});
process.on('exit',()=>{rows.sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)));out.push(rows);console.log(JSON.stringify(out));});
"#);
}

#[test]
fn native_immediate_destroyed_setter_failure_preserves_original_callback_ownership() {
    compare_node_stream_source(r#"
'use strict';
const out=[],marker={};
const immediate=setImmediate(()=>out.push('fired'));
Object.defineProperty(immediate,'_destroyed',{configurable:true,get(){return false},set(value){out.push(['set',value]);throw marker;}});
try{clearImmediate(immediate)}catch(error){out.push(error===marker);}
Object.defineProperty(immediate,'_destroyed',{value:false,writable:true,enumerable:true,configurable:true});
setImmediate(()=>{out.push('rescue');console.log(JSON.stringify(out));process.exit(0);});
"#);
}

#[test]
fn native_timer_custom_promisify_getters_preserve_exports_identity_and_mutation() {
    compare_node_stream_source(r#"
'use strict';
const assert=require('assert'),timers=require('timers'),promises=require('timers/promises'),util=require('util'),out=[];
for(const name of ['setTimeout','setImmediate']){
 assert.strictEqual(timers[name],globalThis[name]);const d=Object.getOwnPropertyDescriptor(timers[name],util.promisify.custom);
 out.push([name,d.enumerable,d.configurable,d.get.name,d.get.length,d.set===undefined,Object.hasOwn(d.get,'prototype'),util.promisify(timers[name])===promises[name],d.get.call(null)===promises[name]]);
 const original=promises[name],marker=function changed(){};promises[name]=marker;out.push(util.promisify(timers[name])===marker);promises[name]=original;
}
Promise.all([util.promisify(timers.setTimeout)(1,'timeout'),util.promisify(timers.setImmediate)('immediate')]).then(values=>{out.push(values);console.log(JSON.stringify(out));});
"#);
}

#[test]
fn native_timer_promise_options_validate_with_original_getters_and_coded_errors() {
    compare_node_stream_source(r#"
'use strict';
const timers=require('timers/promises'),out=[];
(async()=>{
 for(const [label,call] of [
  ['delay',()=>timers.setTimeout(false)],['options',()=>timers.setTimeout(1,null,2)],
  ['signal',()=>timers.setTimeout(1,null,{signal:null})],['ref',()=>timers.setTimeout(1,null,{ref:2})],
  ['immediate-options',()=>timers.setImmediate(null,2)],['immediate-signal',()=>timers.setImmediate(null,{signal:{}})],['immediate-ref',()=>timers.setImmediate(null,{ref:null})]]){
  try{await call();out.push([label,'accepted'])}catch(e){out.push([label,e.name,e.code,e.message,String(e)]);}
 }
 const log=[],options={get signal(){log.push('signal');return undefined},get ref(){log.push('ref');return true}};
 await timers.setTimeout(1,'done',options);out.push(log);
 const marker={};try{await timers.setImmediate(null,{get signal(){throw marker}})}catch(e){out.push(e===marker)}
 console.log(JSON.stringify(out));
})();
"#);
}

#[test]
fn cli_print_evaluates_script_statements_and_language_completion_like_node() {
    for source in ["const x=1; x+2", "var x=4; x*3", "'first'; const x=1;", "if(true){7}else{9}", "globalThis.eval=()=>99; 5", "const t=require('timers/promises'); t.setTimeout(1000,null,{ref:false}).then(()=>{throw Error('unreferenced timer fired')});"] {
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command=Command::new(executable);
            command.args(["-pe",source]);
            if executable!="node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1"); }
            quench_node_test::case_process::observe_command(command,Duration::from_secs(5)).unwrap()
        });
        for observation in &observations { assert!(!observation.timed_out); assert_eq!(observation.exit_code,Some(0),"{}",String::from_utf8_lossy(&observation.stderr)); assert!(observation.stderr.is_empty()); }
        if !source.contains("t.setTimeout") { assert_eq!(observations[0].stdout,observations[1].stdout); }
    }
}

#[test]
fn native_timer_primitive_aliases_cancel_original_handles_like_node() {
    compare_node_stream_source(r#"
"use strict";
const assert=require('assert'),out=[],fired=[];
const a=setTimeout(()=>fired.push('number'),10),b=setInterval(()=>fired.push('string'),10);
const proto=Object.getPrototypeOf(a),method=proto[Symbol.toPrimitive],d=Object.getOwnPropertyDescriptor(proto,Symbol.toPrimitive);
out.push([method.name,method.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(method,'prototype')]);
const primitive=Object.getOwnPropertySymbols(a).find(s=>s.description==='kHasPrimitive');
const asyncId=Object.getOwnPropertySymbols(a).find(s=>s.description==='asyncId');
out.push(a[primitive],typeof a[asyncId]);
const id=+a;out.push(typeof id,Number.isNaN(id),id===method.call(a),String(a)===String(id),a[primitive]);
clearTimeout(id);clearInterval(String(b));out.push(a._destroyed,b._destroyed);
const c=setTimeout(()=>fired.push('uncoerced'),1);clearTimeout(c[asyncId]);
const e=setTimeout(()=>fired.push('getter'),10),original=e[asyncId],log=[];
Object.defineProperty(e,asyncId,{configurable:true,get(){log.push('id');return original;}});
Object.defineProperty(e,primitive,{configurable:true,get(){log.push('flag');return false;},set(v){log.push(['set',v]);}});
out.push(method.call(e)===original,log.slice());clearTimeout(original);
const marker={},f=setTimeout(()=>fired.push('throw'),10);
Object.defineProperty(f,primitive,{configurable:true,get(){throw marker;}});
try{method.call(f)}catch(error){out.push(error===marker)}
Object.defineProperty(f,primitive,{configurable:true,value:false,writable:true});clearTimeout(f);
setTimeout(()=>{out.push(fired);console.log(JSON.stringify(out));},30);
"#);
}

#[test]
fn native_timer_duration_coercion_warning_policy_and_overflow_delay_match_node() {
    compare_node_stream_source(r#"
"use strict";
const out=[],log=[],noop=()=>{};
process.emitWarning=function(message,name){out.push([this===process,arguments.length,message,name]);};
globalThis.String=()=>"wrong conversion";
for(const delay of [undefined,null,0,false,NaN,NaN,-1,-2,Infinity,2147483648,2147483647,'0x10']) { const t=setTimeout(noop,delay);clearTimeout(t); }
const delay={ [Symbol.toPrimitive](hint){log.push(hint);return 0;} };clearInterval(setInterval(noop,delay));out.push(log);
const marker={};try{setTimeout(noop,{[Symbol.toPrimitive](){throw marker;}});}catch(error){out.push(error===marker);}
for(const delay of [Symbol('x'),1n]) {try{setTimeout(noop,delay);}catch(error){out.push(error.name);}}
process.emitWarning=()=>{throw marker;};try{setTimeout(noop,Infinity);}catch(error){out.push(error===marker);}
let fired=false;process.emitWarning=()=>{};setTimeout(()=>{fired=true;},2147483648);setTimeout(()=>{out.push(fired);console.log(JSON.stringify(out));},30);
"#);
}

#[test]
fn native_timer_refresh_reactivates_original_slots_and_repeat_transitions_like_node() {
    compare_node_stream_source(r#"
"use strict";
const assert=require('assert'),out=[],fired=[];
const cleared=setTimeout(()=>fired.push('cleared'),1);clearTimeout(cleared);out.push(cleared.refresh()===cleared,cleared._idleTimeout,cleared._onTimeout);
const replaced=setTimeout(()=>assert.fail('old callback invoked'),1,'old');replaced._timerArgs=['new'];replaced._onTimeout=function(arg){out.push(this===replaced,arg);};
let count=0,oldId;
const timer=setTimeout(function(arg){
 count++;out.push(['phase',count,this===timer,arg,timer._destroyed]);
 if(count===1){out.push(timer.refresh()===timer);}
 if(count===2){process.nextTick(()=>{out.push(['completed',timer._destroyed,typeof timer._onTimeout]);const key=Object.getOwnPropertySymbols(timer).find(s=>s.description==='asyncId');oldId=timer[key];let assigned;const marker={};Object.defineProperty(timer,key,{configurable:true,get(){throw marker;},set(value){assigned=value;}});out.push(timer.refresh()===timer,assigned!==oldId,timer._destroyed);Object.defineProperty(timer,key,{configurable:true,writable:true,enumerable:true,value:assigned});});}
 if(count===3){clearTimeout(timer);out.push(['cancel',timer._destroyed,timer._idleTimeout,timer._onTimeout]);}
},1,'value');
const unref=setTimeout(()=>fired.push('unref'),1);unref.unref();out.push(unref.refresh()===unref,unref.hasRef());
let switched=0;const switchTimer=setTimeout(()=>{switched++;if(switched===1){switchTimer._repeat=1;}else{clearInterval(switchTimer);}},1);
const d=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(timer),'refresh');out.push([d.value.name,d.value.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(d.value,'prototype')]);
setTimeout(()=>{out.push(['end',count,switched,fired,timer._destroyed,typeof timer._onTimeout]);console.log(JSON.stringify(out));},30);
"#);
}

#[test]
fn native_internal_unref_timers_and_promise_init_reference_state_match_node() {
    let source = r#"
"use strict";
const internal=require('internal/timers'),hooks=require('async_hooks'),out=[],phases=[];
out.push(internal===require('internal/timers'),internal.TIMEOUT_MAX,internal.setUnrefTimeout.name,internal.setUnrefTimeout.length,Object.hasOwn(internal.setUnrefTimeout,'prototype'));
const hook=hooks.createHook({init(id,type,trigger,resource){if(type==='Timeout')phases.push([type,resource.hasRef()]);}}).enable();
const t=internal.setUnrefTimeout(function(){out.push(['internal',this===t,arguments.length,this.hasRef()]);},1,'ignored');
require('timers/promises').setTimeout(1,'value',{ref:false}).then(value=>out.push(['promise',value]));
setTimeout(()=>{hook.disable();out.push(phases);console.log(JSON.stringify(out));},30);
"#;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["--expose-internals", "--no-warnings", "-e", source]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(5)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty());
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn native_internal_timer_require_and_resolution_obey_exposure_flag() {
    let source=r#"for(const operation of [()=>require('internal/timers').setUnrefTimeout,()=>require.resolve('internal/timers')]){try{const value=operation();console.log(JSON.stringify([typeof value,typeof value==='string'?value:value.name]));}catch(error){console.log(JSON.stringify([error.name,error.code,error.message.split('\n')[0]]));}}
"#;
    for expose in [false,true] {
        let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command=Command::new(executable);
            if expose { command.arg("--expose-internals"); }
            command.args(["-e",source]);
            if executable!="node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1"); }
            quench_node_test::case_process::observe_command(command,Duration::from_secs(5)).unwrap()
        });
        for observation in &observations { assert!(!observation.timed_out); assert_eq!(observation.exit_code,Some(0)); assert!(observation.stderr.is_empty()); }
        assert_eq!(observations[0].stdout,observations[1].stdout);
    }
}

#[test]
fn native_internal_timer_clock_preserves_metadata_and_monotonic_origin() {
    let source = r#"
"use strict";
const {internalBinding}=require('internal/test/binding'),binding=internalBinding('timers'),fn=binding.getLibuvNow,out=[];
const d=Object.getOwnPropertyDescriptor(binding,'getLibuvNow');out.push(binding===internalBinding('timers'),fn.name,fn.length,Object.hasOwn(fn,'prototype'),d.writable,d.enumerable,d.configurable);
for(const receiver of [undefined,{},binding]){try{const value=fn.call(receiver);out.push(['value',Number.isInteger(value),value>=0,value<0x3ffffff]);}catch(error){out.push([error.name,error.code||null,error.message]);}}
process.uptime=()=>99999999;const begin=binding.getLibuvNow();while(binding.getLibuvNow()-begin<2){}out.push(binding.getLibuvNow()-begin>=2);console.log(JSON.stringify(out));
"#;
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command=Command::new(executable);
        command.args(["--expose-internals","--no-warnings","-e",source]);
        if executable!="node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1"); }
        quench_node_test::case_process::observe_command(command,Duration::from_secs(5)).unwrap()
    });
    for observation in &observations { assert!(!observation.timed_out); assert_eq!(observation.exit_code,Some(0),"{}",String::from_utf8_lossy(&observation.stderr)); assert!(observation.stderr.is_empty()); }
    assert_eq!(observations[0].stdout,observations[1].stdout);
}

#[test]
fn native_buffer_search_matches_node_offsets_bounds_encodings_and_original_hooks() {
    compare_node_stream_source(r#"
"use strict";
const out=[],buffer=Buffer.from('abcabc');
for(const name of ['indexOf','lastIndexOf','includes']) {
 const d=Object.getOwnPropertyDescriptor(Buffer.prototype,name);out.push([name,d.value.name,d.value.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(d.value,'prototype')]);
 for(const value of ['a','bc','',Buffer.from('bc'),new Uint8Array([99]),97,NaN,-1])
  for(const offset of [undefined,-Infinity,-10,-1,-0.5,0,1,2.5,5,Infinity,NaN,null])
   for(const end of [undefined,0,2,4,100,-1,'utf8']) out.push(buffer[name](value,offset,end));
 const log=[],offset={[Symbol.toPrimitive](hint){log.push(hint);return 1;}};out.push(buffer[name]('a',offset),log);
 const marker={};try{buffer[name]('a',{valueOf(){throw marker;}});}catch(error){out.push(error===marker);}
 for(const value of [()=>{}, {}, [], null,undefined,true,Symbol('x'),1n]){try{buffer[name](value);out.push('accepted');}catch(error){out.push([error.name,error.code,error.message]);}}
 for(const call of [()=>buffer[name]({},0),()=>buffer[name]('a',0,'unknown'),()=>Buffer.prototype[name].call({},'a'),()=>buffer[name]('a',Symbol('x')),()=>buffer[name]('a',1n)]) {try{call();out.push('accepted');}catch(error){out.push([error.name,error.code||null]);}}
}
for(const name of ['indexOf','lastIndexOf','includes']) {
 const b=Buffer.from('aβaβ','utf16le');
 for(const needle of ['a','β','',Buffer.from('β','utf16le')])
  for(const offset of [-10,-1,0,1,2,3,5,20]) for(const end of [undefined,0,3,7,100]) out.push(b[name](needle,offset,end,'utf16le'));
 out.push(buffer[name](Buffer.from('a'),0,undefined,'unknown'));
 const encoding={valueOf(){return 'utf8';}};out.push(buffer[name]('a',0,undefined,encoding));
 const b2=Buffer.from('xxx');out.push(b2[name]('a',{valueOf(){b2[1]=97;return 0;}}));
}
const aligned=Buffer.from('aβaβ','utf16le');
out.push(aligned.indexOf('a',1,undefined,'utf16le'));
out.push(aligned.indexOf(Buffer.from('a','utf16le'),1,undefined,'utf16le'));
out.push(aligned.includes('a',5,undefined,'utf16le'));
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_backing_getters_preserve_descriptors_membership_and_original_hooks() {
    compare_node_stream_source(r#"
"use strict";
const out=[];
for(const key of ['parent','offset']){
 const d=Object.getOwnPropertyDescriptor(Buffer.prototype,key),get=d.get;
 out.push([key,d.enumerable,d.configurable,d.set===undefined,get.name,get.length,Object.hasOwn(get,'prototype')]);
 const b=Buffer.from(new ArrayBuffer(12),3,5);
 out.push(key==='parent'?get.call(b)===b.buffer:get.call(b));
 for(const value of [Buffer.prototype,new Uint8Array(3),new DataView(new ArrayBuffer(4)),{},null,undefined]){try{out.push(get.call(value)===undefined);}catch(e){out.push([e.name,e.message]);}}
 const log=[],marker={};Object.defineProperty(b,key==='parent'?'buffer':'byteOffset',{get(){log.push('get');return marker;}});out.push(get.call(b)===marker,log);
 const fake=Object.create(Buffer.prototype);Object.defineProperty(fake,key==='parent'?'buffer':'byteOffset',{value:marker});out.push(get.call(fake)===marker);
}
const get=Object.getOwnPropertyDescriptor(Buffer.prototype,'parent').get,log=[],marker={},target={buffer:marker};
Object.defineProperty(Buffer,Symbol.hasInstance,{configurable:true,value(value){log.push(this===Buffer,value===target);return true;}});out.push(get.call(target)===marker,log);
Object.defineProperty(Buffer,Symbol.hasInstance,{configurable:true,value(){throw marker;}});try{get.call(target);out.push('accepted');}catch(e){out.push(e===marker);}delete Buffer[Symbol.hasInstance];
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_bigint_methods_preserve_layout_original_hooks_errors_and_aliases() {
    compare_node_stream_source(r#"
'use strict';
const out=[],names=[];
for(const action of ['read','write'])for(const type of ['BigUInt64','BigInt64'])for(const end of ['LE','BE'])names.push(action+type+end);
function attempt(fn){try{const value=fn();return ['ok',typeof value==='bigint'?value.toString()+'n':value];}catch(e){return [e.name,e.code||null,e.code?e.message:null];}}
for(const name of names){
 const method=Buffer.prototype[name],d=Object.getOwnPropertyDescriptor(Buffer.prototype,name),read=name.startsWith('read');
 out.push([name,method.name,method.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(method,'prototype'),method===Buffer.prototype[name.replace('UInt','Uint')]]);
 for(const offset of [undefined,0,1,-1,.5,NaN,Infinity,'0',null,1n,{}]){
  const b=Buffer.from([128,1,2,3,4,5,6,255,7]);out.push(attempt(()=>read?method.call(b,offset):method.call(b,123456789n,offset)),Array.from(b));
 }
 if(!read)for(const value of [0n,1n,-1n,-(1n<<63n),(1n<<63n)-1n,(1n<<64n)-1n,1n<<64n,123,'bad',true,null,undefined]){const b=Buffer.alloc(8);out.push(attempt(()=>method.call(b,value)),Array.from(b));}
 const log=[],receiver={length:8};for(let i=0;i<8;i++)Object.defineProperty(receiver,i,{get(){log.push(['get',i]);return (i+1)*241;},set(value){log.push(['set',i,value]);}});
 out.push(attempt(()=>read?method.call(receiver):method.call(receiver,name.includes('UInt')?0xfedcba9876543210n:-0x0123456789abcdefn)),log);
 const typed=new Uint8Array(8);out.push(attempt(()=>read?method.call(typed):method.call(typed,42n)),Array.from(typed));
 const base=Buffer.alloc(12),slice=base.subarray(2,10);out.push(attempt(()=>read?method.call(slice):method.call(slice,42n)),Array.from(base));
 const marker={},short={0:1,get length(){throw marker;}};try{read?method.call(short):method.call(short,1n);out.push(false);}catch(e){out.push(e===marker);}
 if(!read){const log=[],b=Buffer.alloc(8),value={[Symbol.toPrimitive](hint){log.push(hint);return 42n;}};out.push(attempt(()=>method.call(b,value)),Array.from(b),log);}
}
for(const name of names.filter(name=>name.startsWith('write'))){
 const log=[],buffer=Buffer.alloc(8);let count=0;const value={[Symbol.toPrimitive](hint){log.push([hint,Array.from(buffer)]);return ++count<4?0x0123456789abcdefn:0xfedcba9876543210n;}};out.push(attempt(()=>Buffer.prototype[name].call(buffer,value)),Array.from(buffer),log);
 const marker={},partial=Buffer.alloc(8);let calls=0;const throwing={[Symbol.toPrimitive](){if(++calls===4)throw marker;return 42n;}};try{Buffer.prototype[name].call(partial,throwing);out.push(false);}catch(error){out.push(error===marker,Array.from(partial));}
}
const original=globalThis.BigInt;globalThis.BigInt=()=>{throw Error('public BigInt must not replace internal conversion');};out.push(Buffer.from([1,0,0,0,0,0,0,0]).readBigUInt64LE().toString());globalThis.BigInt=original;
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_swaps_preserve_generic_views_thresholds_aliases_and_backing_identity() {
    compare_node_stream_source(r#"
'use strict';
const out=[];
for(const name of ['swap16','swap32','swap64']){
 const d=Object.getOwnPropertyDescriptor(Buffer.prototype,name),method=d.value;
 out.push([name,method.name,method.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(method,'prototype')]);
 for(const length of [0,1,2,4,8,16,24,32,40,48,64,128])for(const Ctor of [Uint8Array,Int8Array,Uint16Array,Uint32Array,BigUint64Array]){
  const view=new Ctor(length);for(let i=0;i<length;i++)view[i]=Ctor===BigUint64Array?BigInt(i+1):(i+1)*257+3;
  try{out.push([Ctor.name,length,method.call(view)===view,Array.from(new Uint8Array(view.buffer))]);}catch(e){out.push([Ctor.name,length,e.name,e.code||null,e.code?e.message:null]);}
 }
 const b=Buffer.from([1,2,3,4,5,6,7,8,9,10]),slice=b.subarray(1,9);try{out.push(method.call(slice)===slice,Array.from(b));}catch(e){out.push(e.name,e.code);}
 for(const value of [{length:8},new DataView(new ArrayBuffer(8)),null,undefined,new Proxy(new Uint8Array(8),{})]){try{method.call(value);out.push('accepted');}catch(e){out.push([e.name,e.code||null]);}}
 const shadow=new Uint8Array(64);for(let i=0;i<shadow.length;i++)shadow[i]=i;Object.defineProperties(shadow,{length:{get(){throw Error('shadow length');}},buffer:{get(){throw Error('shadow buffer');}},byteOffset:{get(){throw Error('shadow offset');}},byteLength:{get(){throw Error('shadow byteLength');}}});try{out.push(method.call(shadow)===shadow,Array.from(shadow));}catch(e){out.push(e.message);}
 const transferred=new Uint8Array(8);structuredClone(transferred.buffer,{transfer:[transferred.buffer]});try{out.push(method.call(transferred)===transferred);}catch(e){out.push([e.name,e.code||null]);}
}
const locale=Object.getOwnPropertyDescriptor(Buffer.prototype,'toLocaleString');out.push(locale.value===Buffer.prototype.toString,locale.enumerable,locale.writable,locale.configurable,Buffer.from('aβ').toLocaleString());
console.log(JSON.stringify(out));
"#);
}

#[test]
fn stream_once_readable_pipe_preserves_original_resume_calls_and_completion() {
    compare_node_stream_source(r#"
'use strict';
const {Readable,Writable}=require('stream'),out=[];
for(const placement of ['before','after']){
 const result={placement,readable:0,finish:0,data:'',resumes:0};out.push(result);
 const source=new Readable({read(){}}),destination=new Writable({write(chunk,encoding,callback){result.data+=chunk;callback();}});
 const resume=source.resume;source.resume=function(...args){result.resumes++;return resume.apply(this,args);};
 if(placement==='before')source.once('readable',()=>result.readable++);
 source.pipe(destination);for(const chunk of ['foo','bar','baz',null])source.push(chunk);
 if(placement==='after')source.once('readable',()=>result.readable++);
 destination.on('finish',()=>result.finish++);
}
process.on('exit',()=>console.log(JSON.stringify(out)));
"#);
}

#[test]
fn native_buffer_constructor_preserves_calls_species_original_methods_and_warning_identity() {
    compare_node_stream_source(r#"
'use strict';
const {Buffer:B}=require('buffer'),out=[],warnings=[];
const originalWarning=process.emitWarning;process.emitWarning=function(...args){warnings.push([this===process,args]);};
const p=Object.getOwnPropertyDescriptor(B,'prototype'),s=Object.getOwnPropertyDescriptor(B,Symbol.species);
out.push([B.name,B.length,p.writable,p.enumerable,p.configurable,Object.getPrototypeOf(B)===Uint8Array,B.prototype.constructor===B,B[Symbol.species]===B,B[Symbol.species].name,B[Symbol.species].prototype===B.prototype,s.enumerable,s.configurable,s.get.name,s.get.length,Object.hasOwn(s.get,'prototype')]);
for(const args of [[3],['abc'],[[1,2,255]],[new ArrayBuffer(5),1,3]])for(const construct of [false,true]){
 const value=construct?new B(...args):B(...args);out.push([construct,Array.from(value),value instanceof B,value instanceof Uint8Array,value.constructor===B,B.isBuffer(value)]);
}
try{new B(42,'utf8');}catch(e){out.push([e.name,e.code,e.message]);}
class Derived extends B{}const derived=new Derived([4,5]);out.push([derived instanceof Derived,derived instanceof B,derived.constructor===B,Array.from(derived)]);
const marker={},from=B.from,alloc=B.alloc;
B.from=function(...args){out.push(['from',this===B,args.length,args[0],args[1],args[2]]);return marker;};out.push(B('input','utf8',2)===marker,new B('input')===marker);B.from=from;
B.alloc=function(...args){out.push(['alloc',this===B,args]);return marker;};out.push(B(3)===marker,new B(3)===marker);B.alloc=alloc;
const real=B.from([1]);Object.defineProperty(B,Symbol.hasInstance,{configurable:true,value(value){out.push(['membership',this===B,value===real]);return true;}});out.push(B.isBuffer(real));delete B[Symbol.hasInstance];
const identity=B.isBuffer;out.push([identity.name,identity.length,Object.hasOwn(identity,'prototype')]);
process.emitWarning=originalWarning;out.push(warnings);console.log(JSON.stringify(out));
"#);
}

#[test]
fn buffer_pool_size_preserves_public_configuration_and_legacy_numeric_construction() {
    compare_node_stream_source(r#"
const B=require('buffer').Buffer,out=[];
const d=Object.getOwnPropertyDescriptor(B,'poolSize');
out.push([d.value,d.writable,d.enumerable,d.configurable]);
const emit=process.emitWarning;process.emitWarning=()=>{};
const b=new B(B.poolSize);out.push([b.length,b.parent instanceof ArrayBuffer]);
B.poolSize=128;out.push(B.poolSize);delete B.poolSize;out.push(Object.hasOwn(B,'poolSize'));
Object.defineProperty(B,'poolSize',d);out.push(B.poolSize);
process.emitWarning=emit;console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_constructor_deprecation_follows_original_vm_filenames_and_process_flags() {
    compare_node_stream_source(r#"
'use strict';
const {spawnSync}=require('child_process'),out=[];
for(const filename of ['/a/node_modules/x.js','/a/node_modules/pkg/node_modules/x.js','/a.js','/node_modules/b.js','c:\\a\\node_modules\\b.js','c:\\a.js','node_modules']){
 for(const pending of [false,true]){
  const source=`const vm=require('vm'),warnings=[];process.emitWarning=function(message,type,code){warnings.push([this===process,message,type,code]);};vm.runInNewContext('Buffer(2);new Buffer(2)',{Buffer},{filename:${JSON.stringify(filename)}});console.log(JSON.stringify(warnings));`;
  const result=spawnSync(process.execPath,[...(pending?['--pending-deprecation']:[]),'-e',source],{encoding:'utf8'});
  out.push([filename,pending,result.status,result.signal,result.stdout,result.stderr]);
 }
}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_of_avoids_legacy_warnings_and_preserves_original_coercions() {
    compare_node_stream_source(r#"
 'use strict';
const B=require('buffer').Buffer,out=[],warnings=[];
process.emitWarning=(...args)=>warnings.push(args);
const f=B.of,d=Object.getOwnPropertyDescriptor(B,'of');
out.push([f.name,f.length,Object.hasOwn(f,'prototype'),d.writable,d.enumerable,d.configurable]);
for(const args of [[],[0,1],[-1,256,257,1.9,NaN,Infinity,undefined,null,'3']]){
 const v=f.call(function(){throw new Error('wrong receiver')},...args);out.push([Array.from(v),v.constructor===B,B.isBuffer(v)]);
}
const hooks=[];const a={valueOf(){hooks.push('a');return 260}},b={valueOf(){hooks.push('b');return -2}};
out.push(Array.from(f(a,b)),hooks);
for(const v of [1n,Symbol('x')])try{f(v)}catch(e){out.push([e.name,e.message])}
try{new f(1)}catch(e){out.push([e.name])}
out.push(warnings);console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_write_preserves_overloads_encodings_bounds_backing_and_hooks() {
 compare_node_stream_source(r#"
'use strict';
const B=require('buffer').Buffer,out=[];
const fn=B.prototype.write,descriptor=Object.getOwnPropertyDescriptor(B.prototype,'write');
out.push([fn.name,fn.length,Object.hasOwn(fn,'prototype'),descriptor.writable,descriptor.enumerable,descriptor.configurable]);
const values=[undefined,null,0,NaN,-1,1.1,Infinity,'abc',1n,Symbol('x'),{}];
const cases=[['abc'],['abc',2,8],['abc',2,9],['abc','utf8'],['abc','utf8',2],['abc',undefined,Symbol('ignored'),'bogus']];
for(const value of values){cases.push(['abc',value],['abc',0,value],['abc',0,2,value],[value]);}
for(const args of cases){const b=B.alloc(8,0x7f);try{out.push(['call',cases.indexOf(args),fn.apply(b,args),Array.from(b)])}catch(e){out.push(['call',cases.indexOf(args),e.name,e.code,e.message,Array.from(b)])}}
for(const encoding of ['utf8','utf16le','hex','base64','latin1','ascii','UTF-8'])for(const value of ['abc','aé😀z','fffff','YWJjZA=='])for(let length=0;length<=8;length++){
 const b=B.alloc(8,0x7f);try{out.push([encoding,value,length,fn.call(b,value,2,length,encoding),Array.from(b)])}catch(e){out.push([encoding,value,length,e.name,e.code,e.message])}
}
for(const receiver of [new Uint8Array(8),new Uint16Array(4),new BigUint64Array(1),new DataView(new ArrayBuffer(8)),{},null])try{out.push(['receiver',fn.call(receiver,'ab',0,2),ArrayBuffer.isView(receiver)?Array.from(new Uint8Array(receiver.buffer)):null])}catch(e){out.push(['receiver',e.name,e.code,e.message])}
const hooks=[];const b=B.alloc(8);Object.defineProperty(b,'length',{get(){throw new Error('shadow length')}});Object.defineProperty(b,'buffer',{get(){throw new Error('shadow buffer')}});Object.defineProperty(b,'byteOffset',{get(){throw new Error('shadow offset')}});
const encoding={[Symbol.toPrimitive](hint){hooks.push(hint);return 'utf8'}};try{out.push(['hooks',fn.call(b,'abc',1,5,encoding),hooks,Array.from(b)])}catch(e){out.push(['hooks',e.name,e.message,hooks])}

for(const enc of ['utf8','ascii','latin1','utf16le','hex','base64']){
 const value=enc==='hex'?'6162':enc==='base64'?'YWI=':'ab';
 const b=B.alloc(8),events=[];Object.defineProperty(b,'byteLength',{get(){events.push('byteLength');return {valueOf(){events.push('number');return 8}}}});
 try{out.push(['byte-length-hooks',enc,fn.call(b,value,1,4,enc),events,Array.from(b)])}catch(e){out.push(['byte-length-hooks',enc,e.name,e.code,e.message,events])}
}
{const b=B.alloc(2);structuredClone(b.buffer,{transfer:[b.buffer]});try{out.push(['detached',fn.call(b,'abc')])}catch(e){out.push(['detached',e.name,e.code,e.message])}}

for(const mode of ['primitive','string','inspect']){
 const events=[];const encoding=mode==='primitive'?{[Symbol.toPrimitive](hint){events.push(hint);return 'unknown'}}:mode==='string'?{valueOf(){events.push('valueOf');return 'unknown'},toString(){events.push('toString');return 'display'}}:{valueOf(){events.push('valueOf');return 'unknown'},[Symbol.for('nodejs.util.inspect.custom')](){events.push('inspect');return 'custom'}};
 try{out.push(['unknown-hooks',mode,fn.call(B.alloc(8),'x',0,4,encoding),events])}catch(e){out.push(['unknown-hooks',mode,e.name,e.code,e.message,events])}
}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_util_string_format_preserves_primitives_builtin_objects_and_original_hooks() {
compare_node_stream_source(r#"
'use strict';
const u=require('util'),out=[],hooks=[];
class Custom{toString(){hooks.push('custom');return 'custom-string'}}
globalThis.Custom=Custom;
const values=[undefined,null,true,false,0,-0,NaN,Infinity,1n,Symbol('x'),'str',{},[],[1,[2]],new Date(0),/abc/,new Number(3),new String('abc'),new Custom(),{[Symbol.toPrimitive](hint){hooks.push(hint);return 'primitive'}},{[Symbol.for('nodejs.util.inspect.custom')](){hooks.push('inspect');return 'inspected'}}];
for(const value of values)try{out.push(u.format('%s',value))}catch(e){out.push([e.name,e.message])}
for(const value of [new Date(0),new Date(NaN),new Number(3),new String('abc'),new Boolean(false),Object(2n),Object(Symbol('boxed'))]){
 value.extra={nested:1};Object.defineProperty(value,'hidden',{value:7});
 out.push(u.format('%s',value));
}
out.push(hooks);console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_fill_preserves_overloads_views_ranges_coercions_and_overlap() {
compare_node_stream_source(r#"
'use strict';const B=require('buffer').Buffer,out=[],fn=B.prototype.fill,d=Object.getOwnPropertyDescriptor(B.prototype,'fill');out.push([fn.name,fn.length,Object.hasOwn(fn,'prototype'),d.writable,d.enumerable,d.configurable]);
const values=[undefined,null,true,false,0,-1,257,NaN,Infinity,1n,Symbol('v'),'', 'a','é','😀','yKJh','\u0222',B.from([1,2]),new Uint8Array([3,4]),new Uint16Array([0x1234,0x5678]),new DataView(new ArrayBuffer(2)),new ArrayBuffer(2),[],{}, {valueOf(){return 7}}];
for(let i=0;i<values.length;i++)for(const tail of [[],[1,4],[1,4,'utf8'],['hex'],['base64'],[0,0,'bogus'],[9,10]]){const b=B.alloc(8,0x7f);try{out.push([i,tail,fn.call(b,values[i],...tail)===b,Array.from(b)])}catch(e){out.push([i,tail,e.name,e.code,e.message,Array.from(b)])}}
for(const name of ['offset','end','encoding'])for(const value of [null,'2',NaN,1.1,-1,9,Infinity,1n,{},true]){const b=B.alloc(8,0x7f),args=name==='offset'?['ab',value,4]:name==='end'?['ab',1,value]:['ab',1,4,value];try{out.push([name,typeof value,String(value),fn.apply(b,args)===b,Array.from(b)])}catch(e){out.push([name,typeof value,String(value),e.name,e.code,e.message])}}
for(const receiver of [new Uint8Array(8),new Uint16Array(4),{},null])try{out.push(['receiver',fn.call(receiver,'ab',1,3)===receiver,Array.from(receiver)])}catch(e){out.push(['receiver',e.name,e.code,e.message])}
const b=B.from([1,2,3,4,5,6,7,8]);out.push(['overlap',fn.call(b,b.subarray(0,3),1,8)===b,Array.from(b)]);

for(const value of [B.alloc(0),new Uint8Array(0),new DataView(new ArrayBuffer(0)),new DataView(new Uint8Array([3,4]).buffer),new Uint8Array([5,6]).buffer,new String('7')]){const b=B.alloc(8,0x7f);try{out.push(['additional-value',Object.prototype.toString.call(value),fn.call(b,value)===b,Array.from(b)])}catch(e){out.push(['additional-value',Object.prototype.toString.call(value),e.name,e.code,e.message,Array.from(b)])}}
for(const length of [2,4,9]){const events=[],encoding={length,toString(){events.push('string');return 'utf8'}};const b=B.alloc(8);try{out.push(['encoding-hooks',length,fn.call(b,'ab',1,4,encoding)===b,events,Array.from(b)])}catch(e){out.push(['encoding-hooks',length,e.name,e.code,e.message,events])}}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_binding_fill_preserves_statuses_indices_and_shared_byte_policy() {
compare_node_stream_source(r#"
'use strict';
const {spawnSync}=require('child_process');
const source=`process.emitWarning=()=>{};const internal=require('internal/test/binding').internalBinding,binding=internal('buffer'),f=binding.fill,out=[];out.push(binding===internal('buffer'));out.push([f.name,f.length,Object.hasOwn(f,'prototype')]);for(const args of [[1,-1,0,1],[1,0,-2,1],[1,0,3,1],['ab',0,4,'utf8'],['x',0,4,'hex'],[Buffer.alloc(0),0,4,'utf8'],[true,1,4,'utf8'],[new Uint8Array([2,3]),1,7,'utf8']]){const b=Buffer.alloc(8,0x7f);try{const value=f(b,...args);out.push([value===undefined?'undefined':value,Array.from(b)])}catch(e){out.push([e.name,e.code,e.message,Array.from(b)])}}console.log(JSON.stringify(out));`;
const result=spawnSync(process.execPath,['--expose-internals','-e',source],{encoding:'utf8'});console.log(JSON.stringify([result.status,result.signal,result.stdout,result.stderr]));
"#);
}

#[test]
fn native_buffer_inspection_preserves_aliases_limits_properties_and_constructor_errors() {
compare_node_stream_source(r#"
'use strict';const mod=require('buffer'),B=mod.Buffer,u=require('util'),out=[],custom=Symbol.for('nodejs.util.inspect.custom');
for(const key of ['inspect',custom]){const d=Object.getOwnPropertyDescriptor(B.prototype,key);out.push([String(key),d.value.name,d.value.length,Object.hasOwn(d.value,'prototype'),d.writable,d.enumerable,d.configurable])}
{const d=Object.getOwnPropertyDescriptor(mod,'INSPECT_MAX_BYTES');out.push([d.get.name,d.get.length,d.set.name,d.set.length,d.enumerable,d.configurable,B.prototype.inspect===B.prototype[custom]])}
for(const max of [50,2,Infinity,0,2.5,-0]){mod.INSPECT_MAX_BYTES=max;for(const length of [0,2,4,51]){const b=B.alloc(length,'x');out.push([String(max),length,u.inspect(b),b.inspect()]);}}
mod.INSPECT_MAX_BYTES=50;
for(const value of [NaN,-1,'bad',1n,null,{}])try{mod.INSPECT_MAX_BYTES=value;out.push(['set','accepted'])}catch(e){out.push(['set',e.name,e.code,e.message])}
for(const b of [B.alloc(0),B.from([1,2])]){b.extra=new Uint8Array(0);b.inspect=undefined;out.push(['props',u.inspect(b)]);}
const b=B.from([1,2]);Object.defineProperty(b,'constructor',{get(){throw new Error('ignored')}});out.push(['constructor',u.inspect(b)]);

for(const length of [NaN,undefined,-1,-0.5,2.5,100,Infinity])for(const size of [0,4]){const b=B.alloc(size,1),events=[];Object.defineProperty(b,'length',{get(){events.push('length');return length}});try{out.push(['length-hooks',String(length),size,b.inspect(),events])}catch(e){out.push(['length-hooks',String(length),size,e.name,e.code,e.message,events])}}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_binary_inspection_preserves_view_layouts_backing_and_detached_state() {
compare_node_stream_source(r#"
'use strict';const u=require('util'),out=[];
for(const value of [new Uint8Array(0),new Uint8Array([1,2,255]),new Uint16Array([256,512]),new BigInt64Array([-1n,2n]),new ArrayBuffer(0),new Uint8Array([1,2,3]).buffer,new DataView(new ArrayBuffer(0)),new DataView(new Uint8Array([3,4]).buffer)])out.push(u.inspect(value));
const ab=new ArrayBuffer(2);structuredClone(ab,{transfer:[ab]});out.push(u.inspect(ab));
console.log(JSON.stringify(out));
"#);
}

#[test]
fn http_response_preserves_http10_close_framing_raw_send_and_date_setting() {
    compare_node_stream_source(r#"
'use strict';
const http=require('http'),net=require('net');
let index=0,dateReads=0;
const cases=[['1.0',false,false],['1.0',true,false],['1.0',false,true],['1.1',false,true],['1.0',false,false,false]];
function next(){
 if(index===cases.length)return;
 const [version,split,length,head=true]=cases[index++];dateReads=0;
 const server=http.createServer((req,res)=>{
  const f=res._send;console.log(JSON.stringify(['method',f.name,f.length,Object.hasOwn(f,'prototype'),Object.hasOwn(res,'_send')]));
  Object.defineProperty(res,'sendDate',{get(){dateReads++;return false},configurable:true});
  const headers={'Content-Type':'text/plain'};
  if(length)headers['Content-Length']='6';
  if(head)res.writeHead(200,headers);else for(const key of Object.keys(headers))res.setHeader(key,headers[key]);
  if(split){res.write('hel');res._send('');res.write('lo\n');res._send('');res.end()}
  else res.end('hello\n');
 });
 server.listen(0,()=>{
  const client=net.createConnection(server.address().port);let data='';
  client.setEncoding('utf8');client.on('connect',()=>client.write(`GET / HTTP/${version}\r\nHost: localhost\r\n${version==='1.1'?'Connection: close\r\n':''}\r\n`));
  client.on('data',chunk=>data+=chunk);
  client.on('end',()=>{require('assert').strictEqual(dateReads,1);console.log(JSON.stringify([version,split,length,data]));client.end();server.close(next)});
 });
}
next();
"#);
}

#[test]
fn http_header_validators_preserve_metadata_coercions_errors_and_byte_rules() {
compare_node_stream_source(r#"
'use strict';
const http=require('http'),out=[],hooks=[];
function shape(fn){
 if(typeof fn!=='function')return typeof fn;
 let constructible;try{Reflect.construct(Object,[],fn);constructible=true}catch{constructible=false}
 const properties=Object.getOwnPropertyDescriptors(fn);
 return [fn.name,fn.length,Object.hasOwn(fn,'prototype'),constructible,Object.keys(fn),Object.fromEntries(['name','length','withoutStackTrace'].filter(k=>properties[k]).map(k=>[k,[properties[k].writable,properties[k].enumerable,properties[k].configurable]]))];
}
function invoke(tag,fn,args){
 hooks.length=0;
 try{const result=Reflect.apply(fn,undefined,args);out.push([tag,'return',typeof result,result===undefined,hooks.slice()])}
 catch(e){out.push([tag,'throw',e.name,e.code,e.message,hooks.slice()])}
}
for(const name of ['validateHeaderName','validateHeaderValue']){
 const fn=http[name];out.push([name,shape(fn),shape(fn.withoutStackTrace),fn===fn.withoutStackTrace]);
}
const names=[undefined,null,false,0,1,1n,Symbol('name'),'', 'valid-Name','x space','x\tname','x\x7fname','é','😀','\ud800',new String('boxed'),[],{}, {toString(){hooks.push('name-string');return 'bad name'}},{[Symbol.toPrimitive](hint){hooks.push(['name-hint',hint]);return 'bad name'}}];
const labels=[undefined,null,false,0,'','Custom label',Symbol('label'),{toString(){hooks.push('label-string');return 'Object label'}}];
for(let n=0;n<names.length;n++)for(let l=0;l<labels.length;l++)invoke(['name',n,l],http.validateHeaderName,[names[n],labels[l]]);
for(const [tag,value] of [['string-throw',{toString(){hooks.push('throw');throw new RangeError('hook failed')}}],['primitive-throw',{[Symbol.toPrimitive](hint){hooks.push(hint);throw new RangeError('primitive failed')}}]]){
 invoke(['name-hook',tag],http.validateHeaderName,[value]);
 invoke(['value-hook',tag],http.validateHeaderValue,['x',value]);
}
const values=[undefined,null,false,0,-0,1n,Symbol('value'),'', 'valid','\t','\r','\n','\0','\x7f','é','\u0100','😀','\ud800',new String('boxed'),[],{}, {toString(){hooks.push('value-string');return 'ok'}},{[Symbol.toPrimitive](hint){hooks.push(['value-hint',hint]);return 'ok'}}];
for(let v=0;v<values.length;v++)for(const lenient of [undefined,false,true,0,1,'',{},null])invoke(['value',v,typeof lenient,String(lenient)],http.validateHeaderValue,['x',values[v],lenient]);
for(let code=0;code<=255;code++)for(const lenient of [false,true])invoke(['byte',code,lenient],http.validateHeaderValue,['x',String.fromCharCode(code),lenient]);
for(let n=0;n<names.length;n++)for(const value of [undefined,'\r','ok'])invoke(['error-name',n,value],http.validateHeaderValue,[names[n],value]);
for(const name of ['validateHeaderName','validateHeaderValue'])for(const args of [[],['valid','ok'],['bad name','\r']])invoke(['unwrapped',name,args],http[name].withoutStackTrace,args);
console.log(JSON.stringify(out));
"#);
}

#[test]
fn cli_eval_builtin_globals_preserve_lazy_descriptors_cache_and_assignment() {
compare_node_stream_source(r#"
'use strict';
const out=[];
function descriptor(name){const d=Object.getOwnPropertyDescriptor(globalThis,name);return !d?null:[d.enumerable,d.configurable,typeof d.get,typeof d.set,d.get&&[d.get.name,d.get.length,Object.hasOwn(d.get,'prototype')],d.set&&[d.set.name,d.set.length,Object.hasOwn(d.set,'prototype')]]}
for(const name of ['vm','fs','path','util','buffer','fs/promises','_http_agent'])out.push(['initial',name,descriptor(name)]);
const before=Object.getOwnPropertyDescriptor(globalThis,'vm');
try{const value=vm;const after=Object.getOwnPropertyDescriptor(globalThis,'vm');out.push(['load',value===require('vm'),descriptor('vm'),before.get===after.get,before.set===after.set])}catch(e){out.push(['load-error',e.name,e.message])}
try{vm=123;out.push(['assign',vm,Object.getOwnPropertyDescriptor(globalThis,'vm')])}catch(e){out.push(['assign-error',e.name,e.message])}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn cli_eval_builtin_globals_use_original_module_identity_and_mutable_require_hook() {
compare_node_stream_source(r#"
'use strict';
const M=require('module'),original=M.prototype.require,out=[];
M.prototype.require=function(name){out.push(['require-hook',name,this.id,this.filename,this.loaded]);if(name==='vm')return {fromHook:1};return original.call(this,name)};
try{const first=vm;M.prototype.require=function(){throw new Error('second require must not run')};out.push(['cached',first===vm,first.fromHook])}catch(e){out.push(['error',e.name,e.message])}
M.prototype.require=original;
console.log(JSON.stringify(out));
"#);
}

#[test]
fn cli_eval_builtin_accessors_preserve_blocked_cache_inherited_setters_and_errors() {
compare_node_stream_source(r#"
'use strict';
const M=require('module'),original=M.prototype.require,out=[];
let calls=0;
M.prototype.require=function(name){if(name==='fs'){calls++;return {calls}}return original.call(this,name)};
Object.defineProperty(globalThis,'fs',{configurable:false});
const a=fs,b=fs;out.push(['blocked-cache',a.calls,b.calls,a===b]);
try{fs=1}catch(e){out.push(['blocked-set',e.name,e.message])}
M.prototype.require=original;
let seen=[];
Object.defineProperty(Object.getPrototypeOf(globalThis),'path',{set(v){seen.push([this===globalThis,v])},configurable:true});
path=4;out.push(['inherited-setter',seen,Object.hasOwn(globalThis,'path')]);
delete Object.getPrototypeOf(globalThis).path;
const marker={};M.prototype.require=function(){throw marker};
try{void vm}catch(e){out.push(['original-error',e===marker])}
M.prototype.require=original;
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_from_and_copy_bytes_preserve_conversion_bounds_brands_and_hooks() {
compare_node_stream_source(r#"
'use strict';const B=Buffer,out=[],hooks=[];
function check(tag,value,...tail){hooks.length=0;try{const b=B.from(value,...tail);out.push([tag,'ok',Array.from(b),Object.getPrototypeOf(b)===B.prototype,b.buffer===value,hooks.slice()])}catch(e){out.push([tag,e.name,e.code,e.message,hooks.slice()])}}
for(const fn of [B.from,B.copyBytesFrom])out.push(['shape',fn&&fn.name,fn&&fn.length,!!fn&&Object.hasOwn(fn,'prototype')]);
const values=[undefined,null,false,0,1,1n,Symbol('v'),{},Object.create(null),new Boolean(true),new Number(3),new String('test'),{valueOf(){hooks.push('valueOf');return 'test'}},{[Symbol.toPrimitive](hint){hooks.push(hint);return 'test'}},{valueOf(){hooks.push('valueOf');return null}},{valueOf:null},{valueOf:1},{length:3,0:257,1:-1,2:2},{length:'3',0:1},{length:NaN},{type:'Buffer',data:[1,2,3]},new Uint8Array([1,2,255]),new Uint16Array([0xffff,256]),new BigInt64Array([1n]),new DataView(new ArrayBuffer(3)),[1,2,257],['1','2',null]];
for(let i=0;i<values.length;i++)check(['value',i],values[i]);
for(const value of ['test','é','😀','\ud800','aaZZbb'])for(const enc of [undefined,null,false,0,{},'utf8','utf16le','latin1','hex','base64','bogus'])check(['string',value,typeof enc,String(enc)],value,enc);
for(const offset of [undefined,null,0,1,1.5,-1,6,NaN,Infinity,'2',1n,{valueOf(){hooks.push('offset');return 1}}])for(const length of [undefined,null,0,2,2.5,-1,9,NaN,Infinity,'2',1n])check(['ab',typeof offset,String(offset),typeof length,String(length)],new Uint8Array([1,2,3,4,5]).buffer,offset,length);
for(const kind of ['valueOf','primitive','length','buffer','type','data']){
 let o={};
 if(kind==='valueOf')Object.defineProperty(o,'valueOf',{get(){hooks.push('get-valueOf');return function(){hooks.push('call-valueOf');return 'xy'}}});
 if(kind==='primitive')o[Symbol.toPrimitive]=hint=>{hooks.push(hint);return 'xy'};
 if(kind==='length')Object.defineProperty(o,'length',{get(){hooks.push('length');return 2}});
 if(kind==='buffer'){o.buffer=new ArrayBuffer(2)};
 if(kind==='type'){o.type='Buffer';Object.defineProperty(o,'data',{get(){hooks.push('data');return [3,4]}})};
 if(kind==='data')o={type:'Buffer',data:new Uint8Array([3,4])};
 check(['hooks',kind],o);
}
for(const source of [new Uint8Array([1,2,3]),new Uint16Array([0x1234,0xabcd]),new BigInt64Array([1n,-2n]),new DataView(new ArrayBuffer(4)),[],new Uint8Array(0)])for(const offset of [undefined,null,0,1,-1,NaN,1.5,9,'1',1n])for(const length of [undefined,0,1,9,-1]){
 hooks.length=0;try{const b=B.copyBytesFrom(source,offset,length);out.push(['copy',source.constructor.name,typeof offset,String(offset),length,'ok',Array.from(b)])}catch(e){out.push(['copy',source.constructor.name,typeof offset,String(offset),length,e.name,e.code,e.message])}
}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn inspect_shallow_null_prototype_records_preserve_empty_hidden_and_symbol_keys() {
compare_node_stream_source(r#"
const u=require('util'),out=[],empty=Object.create(null),visible=Object.assign(Object.create(null),{a:1}),hidden=Object.create(null),symbol=Object.create(null),hiddenSymbol=Object.create(null);
Object.defineProperty(hidden,'x',{value:1});symbol[Symbol('x')]=1;Object.defineProperty(hiddenSymbol,Symbol('x'),{value:1});
for(const value of [empty,visible,hidden,symbol,hiddenSymbol])for(const depth of [-2,-1])for(const showHidden of [false,true])out.push(u.inspect(value,{depth,showHidden}));
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_string_conversion_preserves_bounds_metadata_encodings_and_original_hooks() {
compare_node_stream_source(r#"
const B=require('buffer').Buffer,out=[],hooks=[];
function record(tag,fn){hooks.length=0;try{const value=fn();out.push([tag,'ok',typeof value==='string'?Array.from(value,c=>c.codePointAt(0)):value,hooks.slice()])}catch(e){out.push([tag,e.name,e.code,e.message,hooks.slice()])}}
for(const n of ['toString','toLocaleString','asciiSlice','base64Slice','base64urlSlice','latin1Slice','hexSlice','ucs2Slice','utf8Slice']){const f=B.prototype[n];record(['shape',n],()=>[f.name,f.length,Object.hasOwn(f,'prototype'),Object.getOwnPropertyDescriptor(B.prototype,n).enumerable])}
record('alias',()=>B.prototype.toString===B.prototype.toLocaleString);
const b=B.from([0,65,127,128,195,169,255,0,216]);
for(const enc of [undefined,'utf8','hex','base64','base64url','latin1','ascii','utf16le','bogus','\ud800',null,false,0,Symbol('enc'),{toString(){hooks.push('encoding');return 'hex'}}])for(const start of [undefined,-1,0,1,1.5,20,NaN,Infinity,'2',1n])record(['toString',typeof enc,String(enc),typeof start,String(start)],()=>b.toString(enc,start));
for(const start of [undefined,-1,0,2,2.9,20,NaN,Infinity,'2',null,1n,{valueOf(){hooks.push('start');return 2}}])for(const end of [undefined,-1,0,4,4.9,20,NaN,Infinity,'5',null,1n,{valueOf(){hooks.push('end');return 5}}])record(['range',typeof start,String(start),typeof end,String(end)],()=>b.toString('hex',start,end));
for(const value of [{length:0},Object.create(B.prototype),new Uint8Array([65,66]),new Uint16Array([65,66]),new DataView(new ArrayBuffer(2)),null,undefined])for(const args of [[],['hex'],['hex',20],['bogus',0,0]])record(['receiver',Object.prototype.toString.call(value),args],()=>B.prototype.toString.apply(value,args));
for(const n of ['asciiSlice','base64Slice','base64urlSlice','latin1Slice','hexSlice','ucs2Slice','utf8Slice'])for(const start of [undefined,-1,-0.5,-1.5,0,1,1.5,20,NaN,Infinity,-Infinity,2**63,2**64,'2',1n,Symbol('index')])for(const end of [undefined,-1,0,4,20,NaN,'5',1n])record([n,typeof start,String(start),typeof end,String(end)],()=>B.prototype[n].call(b,start,end));
const own=B.from([65,66,67]);Object.defineProperty(own,'length',{get(){hooks.push('length');return 1}});own.subarray=()=>{throw Error('public subarray')};
for(const args of [[],[undefined],['hex'],['hex',0,2]])record(['original-hooks',args],()=>own.toString(...args));
console.log(JSON.stringify(out));
"#);
}

#[test]
fn native_buffer_string_slices_preserve_detached_views_and_original_conversion_errors() {
compare_node_stream_source(r#"
const out=[],marker={},hooks=[];function check(tag,fn){hooks.length=0;try{const v=fn();out.push([tag,'ok',v,hooks.slice()])}catch(e){out.push([tag,e.name,e.code,e.message,e===marker,hooks.slice()])}}
for(const detach of [false,true]){const ab=new ArrayBuffer(4);new Uint8Array(ab).set([65,66,67,68]);const dv=new DataView(ab);if(detach)structuredClone(ab,{transfer:[ab]});for(const method of ['utf8Slice','hexSlice'])check([method,'dv',detach],()=>Buffer.prototype[method].call(dv,{valueOf(){hooks.push('start');throw marker}}));}
const rab=new ArrayBuffer(8,{maxByteLength:16}),dv=new DataView(rab,4,4);rab.resize(2);check('oob-dv',()=>Buffer.prototype.hexSlice.call(dv,0,4));
const b=Buffer.from([65,66,67,68]);check('changing-start',()=>b.toString('hex',{valueOf(){hooks.push('start');return hooks.length}}));
for(const position of ['start','end','encoding'])check(['throw',position],()=>b.toString(position==='encoding'?{toString(){throw marker}}:'hex',position==='start'?{valueOf(){throw marker}}:0,position==='end'?{valueOf(){throw marker}}:4));
const constants=require('buffer').constants;check('limit-export-isolation',()=>{try{constants.MAX_STRING_LENGTH=0}catch{}return b.toString('hex')});for(const name of ['MAX_LENGTH','MAX_STRING_LENGTH'])check(['constant',name],()=>Object.getOwnPropertyDescriptor(constants,name));console.log(JSON.stringify(out));"#);
}

#[test]
fn native_buffer_static_comparison_preserves_brands_detachment_metadata_and_byte_order() {
compare_node_stream_source(r#"
const B=Buffer,out=[];const f=B.compare,d=Object.getOwnPropertyDescriptor(B,'compare');out.push([f.name,f.length,Object.hasOwn(f,'prototype'),d.writable,d.enumerable,d.configurable]);function record(a,b){try{out.push(['ok',f(a,b)])}catch(e){out.push([e.name,e.code,e.message])}};const fake=Object.create(B.prototype),u8=new Uint8Array([1,2]);for(const a of [undefined,null,0,'x',{},fake,new Uint8ClampedArray([1]),new Uint16Array([1]),new DataView(new ArrayBuffer(1)),B.from([]),B.from([1]),u8])for(const b of [undefined,fake,new Uint8Array([]),new Uint8Array([1,3]),u8])record(a,b);const ab=new ArrayBuffer(2),view=new Uint8Array(ab);structuredClone(ab,{transfer:[ab]});record(view,view);record(view,B.alloc(0));for(const left of [{},fake,new Uint8Array([1,2]),new Uint16Array([513]),B.alloc(0)])for(const right of [undefined,fake,new Uint8Array([]),new Uint8Array([1,2])]){try{out.push(['equals','ok',B.prototype.equals.call(left,right)])}catch(e){out.push(['equals',e.name,e.code,e.message])}};const eq=B.prototype.equals;out.push(['equals-shape',eq.name,eq.length,Object.hasOwn(eq,'prototype')]);console.log(JSON.stringify(out));
"#);
}

#[test]
fn node_modules_bounds_package_scope_without_hiding_local_package_types() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("package.json"),
        r#"{"type":"module"}"#,
    )
    .unwrap();
    for (package_type, expected) in [
        (None, "function\n"),
        (Some("module"), "undefined\n"),
        (Some("commonjs"), "function\n"),
    ] {
        let package = directory.path().join("node_modules/fixture");
        std::fs::create_dir_all(&package).unwrap();
        let manifest = package.join("package.json");
        if let Some(package_type) = package_type {
            std::fs::write(&manifest, format!(r#"{{"type":"{package_type}"}}"#)).unwrap();
        }
        std::fs::write(package.join("entry.js"), "console.log(typeof require)").unwrap();
        for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
            let mut command = Command::new(executable);
            command
                .current_dir(directory.path())
                .arg("node_modules/fixture/entry.js");
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            let result =
                quench_node_test::case_process::observe_command(command, Duration::from_secs(10))
                    .unwrap();
            assert_eq!(
                result.exit_code,
                Some(0),
                "{executable}: {:?}",
                result.stderr
            );
            assert_eq!(result.stdout, expected.as_bytes(), "{executable}");
        }
        if manifest.exists() {
            std::fs::remove_file(manifest).unwrap();
        }
    }
}

#[test]
fn binary_http_response_views_match_the_node_oracle() {
    let source = r#"
const http = require('http');
const chunks = [Buffer.from([0,128,255,13,10]), new Uint8Array([9,1,254,8]).subarray(1,3)];
const server = http.Server((req,res) => { for (const chunk of chunks) res.write(chunk); res.write('ff','hex'); res.end(Buffer.from([0,255])); });
server.listen(0,'127.0.0.1',()=>http.get({host:'127.0.0.1',port:server.address().port},response=>{
  const body=[]; response.on('data',chunk=>body.push(chunk)); response.on('end',()=>{
    console.log(Buffer.concat(body).toString('hex'), Math.abs(Date.now()-Date.parse(response.headers.date)) < 5000, Object.getPrototypeOf(response.headers)===Object.prototype); server.close();
  });
}));
"#;
    let mut observations = Vec::new();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let result =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(10))
                .unwrap();
        assert_eq!(
            result.exit_code,
            Some(0),
            "{executable}: {:?}",
            result.stderr
        );
        observations.push(result.stdout);
    }
    assert_eq!(observations[0], b"0080ff0d0a01feff00ff true true\n");
    assert_eq!(observations[0], observations[1]);
}

#[test]
fn headers_and_host_clone_markers_match_the_node_oracle() {
    let source = r#"
const {markAsUncloneable}=require('worker_threads');
const globals=['File','FormData','Headers','Request','Response'].map(name=>{const d=Object.getOwnPropertyDescriptor(globalThis,name);return [name,d.writable,d.enumerable,d.configurable,typeof globalThis[name]]});
const h=new Headers([['X-Test',' a '],['x-test','b'],['set-cookie','a=1'],['set-cookie','b=2']]);
const rows=[globals,[...h],h.get('x-test'),h.has('X-Test'),h.getSetCookie()];
h.delete('X-Test'); rows.push(h.has('x-test'));
for (const [name,args] of [['has',[]],['set',['x']],['append',['a b','c']],['append',['x','a\nb']],['get',[Symbol()]]]) {
  try { h[name](...args); } catch(e) { rows.push([e.name,e.message]); }
}
const target=Object.freeze({x:1}); const before=Reflect.ownKeys(target); markAsUncloneable(target); markAsUncloneable(target);
rows.push(JSON.stringify(before)===JSON.stringify(Reflect.ownKeys(target)));
try { structuredClone({target}); } catch(e) { rows.push([e instanceof DOMException,e.name,e.code,e.message]); }
const getterError=new Error('getter'); const input={get x(){throw getterError}};
try {structuredClone(input)} catch(e) {rows.push(e===getterError)}
let saved;try{structuredClone(target)}catch(e){saved=e}
try{structuredClone({get x(){throw saved}})}catch(e){rows.push(e===saved)}
const array=[1,2];markAsUncloneable(array);rows.push(structuredClone(array));
console.log(JSON.stringify(rows));
"#;
    let mut observations = Vec::new();
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        let result =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(15))
                .unwrap();
        assert_eq!(
            result.exit_code,
            Some(0),
            "{executable}: {:?}",
            result.stderr
        );
        observations.push(result.stdout);
    }
    assert_eq!(observations[0], observations[1]);
}

#[test]
fn native_http_status_reasons_preserve_mutable_map_coercion_errors_and_wire_bytes() {
compare_node_stream_source(r#"
const http=require('http'),net=require('net'),codes=http.STATUS_CODES,marker={},out=[],saved=Object.getOwnPropertyDescriptors(codes);
const initial=[Object.keys(codes).map(k=>[k,codes[k],Object.getOwnPropertyDescriptor(codes,k)]),Object.getOwnPropertyDescriptor(http,'STATUS_CODES').writable,Object.getPrototypeOf(codes)===Object.prototype];
const cases=[
 ['head-default',r=>r.writeHead(200)],
 ['cached-default-map-mutation',r=>{codes[200]='New';r.writeHead(200)}],
 ['custom-message-accessor',(r,events)=>{Object.defineProperty(r,'statusMessage',{configurable:true,get(){events.push('get');return 'Custom'}});r.writeHead(200)}],
 ['implicit-default',r=>{r.statusCode=451;r.end('x')}],
 ['map-mutation',r=>{codes[600]='Changed';r.statusCode=600;r.end('x')}],
 ['map-falsy',r=>{codes[601]=0;r.statusCode=601;r.end('x')}],
 ['map-replacement',r=>{codes[602]='Captured';http.STATUS_CODES={602:'Wrong'};r.writeHead(602)}],
 ['explicit-empty',r=>r.writeHead(200,'')],
 ['existing-empty',r=>{r.statusMessage='';r.writeHead(404)}],
 ['existing-number',r=>{r.statusMessage=123;r.writeHead(200)}],
 ['existing-object',(r,events)=>{r.statusMessage={toString(){events.push('convert');return 'Object Reason'}};r.writeHead(200)}],
 ['latin1',r=>r.writeHead(200,'caf\u00e9')],
 ['invalid-crlf',r=>r.writeHead(201,'bad\r\nInjected: x')],
 ['invalid-wide',r=>r.writeHead(202,'bad\u010d')],
 ['invalid-del',r=>r.writeHead(203,'bad\x7f')],
 ['status-object',r=>r.writeHead({})],
 ['status-array',r=>r.writeHead([])],
 ['status-string',r=>r.writeHead('201')],
 ['status-fraction',r=>r.writeHead(201.7)],
 ['status-wrap',r=>r.writeHead(2**32+200)],
 ['map-throw',r=>{Object.defineProperty(codes,500,{configurable:true,get(){throw marker}});r.writeHead(500)}],
 ['message-accessor',(r,events)=>{let value='';Object.defineProperty(r,'statusMessage',{configurable:true,get(){events.push('get');return value},set(v){events.push(['set',v]);value=v}});r.writeHead(200)}],
];
const server=http.createServer((req,res)=>{const i=Number(req.url.slice(1)),events=[];res.sendDate=false;let error=null;try{cases[i][1](res,events)}catch(e){error=[e.name,e.code,e.message,e===marker]}
 const value=res.statusMessage;out[i]=[cases[i][0],error,res.statusCode,typeof value,typeof value==='string'||typeof value==='number'?value:null,!!res.headersSent,events.slice()];
 http.STATUS_CODES=codes;for(const key of Object.keys(codes))if(!(key in saved))delete codes[key];for(const [k,d]of Object.entries(saved))Object.defineProperty(codes,k,d);
 if(error){res.statusMessage='OK';res.statusCode=200}if(!res.finished)res.end('x');});
server.listen(0,'127.0.0.1',()=>{let i=0;function next(){const parts=[];const socket=net.connect(server.address().port,'127.0.0.1',()=>socket.write(`GET /${i} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n`));socket.on('data',b=>parts.push(b));socket.on('end',()=>{out[i].push(Buffer.concat(parts).toString('latin1').split('\r\n')[0]);if(++i<cases.length)next();else server.close(()=>console.log(JSON.stringify([initial,out])))});}next();});

"#);
}

#[test]
fn native_util_extend_preserves_reverse_copy_effects_metadata_errors_and_deprecation() {
compare_node_stream_source(r#"
const u=require('util'),f=u._extend,out=[],marker={},events=[],original=process.emitWarning;
process.emitWarning=function(...args){events.push(['warning',...args.map(x=>typeof x==='function'?[x.name,x.length]:x)])};
function check(tag,fn){events.length=0;try{out.push([tag,'ok',fn(),events.slice()])}catch(e){out.push([tag,e.name,e.code,e.message,e===marker,events.slice()])}}
check('metadata',()=>{const d=Object.getOwnPropertyDescriptor(u,'_extend'),p=Object.getPrototypeOf(f);return [f.name,f.length,Object.hasOwn(f,'prototype'),p.name,p.length,f.prototype===p.prototype,d.writable,d.enumerable,d.configurable]});
check('no-deprecation',()=>{process.noDeprecation=true;const result=f({},{});delete process.noDeprecation;return result});check('warning-first',()=>f({},{}));check('warning-once',()=>f({},{}));check('shared-warning-code',()=>u.deprecate(()=>42,'duplicate','DEP0060')());
for(const source of [undefined,null,false,1,'abc',Symbol('s'),1n,()=>{},[],[1,2],new Uint8Array([3,4]),Object('ab'),{x:1}])check(['source',typeof source,String(source)],()=>{const target={};return [f(target,source)===target,target]});
for(const target of [undefined,null,false,1,'abc',Symbol('t'),1n,{},Object.preventExtensions({}),Object.freeze({x:0})])check(['target',typeof target,String(target)],()=>f(target,{x:1}));
check('reverse',()=>{const target={};const source={get a(){events.push('a');return 1},get b(){events.push('b');return 2}};return [f(target,source)===target,Object.keys(target)]});
check('snapshot',()=>{const source={a:1,get b(){events.push('b');delete source.a;source.c=3;return 2}};return f({},source)});
check('symbols',()=>{const source={x:1,[Symbol('key')]:2};Object.defineProperty(source,'hidden',{value:3});return Reflect.ownKeys(f({},source))});
check('setters',()=>{const target={set a(v){events.push(['a',this===target,v])},set b(v){events.push(['b',this===target,v])}};return f(target,{a:1,b:2})===target});
check('getter-throws',()=>f({},{get a(){events.push('get');throw marker}}));
check('setter-throws',()=>f({set a(v){events.push('set');throw marker}},{a:1}));
check('proxy',()=>{const source=new Proxy({a:1,b:2},{ownKeys(t){events.push('keys');return ['a','b']},getOwnPropertyDescriptor(t,k){events.push(['descriptor',k]);return Object.getOwnPropertyDescriptor(t,k)},get(t,k){events.push(['get',k]);return t[k]}});return f({},source)});
check('proxy-false',()=>f(new Proxy({},{set(t,k,v){events.push(['set',k,v]);return false}}),{a:1}));
check('proto',()=>{const source=JSON.parse('{"__proto__":{"inherited":42}}');const target=f({},source);return [target.inherited,Object.hasOwn(target,'__proto__')]});
check('construct',()=>{const source={x:1},target={};return new f(target,source)===target});
process.emitWarning=original;console.log(JSON.stringify(out));

"#);
}

#[test]
fn native_stream_duplex_pair_preserves_metadata_demand_finish_and_peer_destruction() {
compare_node_stream_source(r#"
const {duplexPair,Duplex}=require('stream'),out=[],events=[];const f=duplexPair;out.push(['function',f.name,f.length,Object.hasOwn(f,'prototype'),Object.getOwnPropertyDescriptor(require('stream'),'duplexPair')]);
const [a,b]=f({objectMode:true,highWaterMark:2});const proto=Object.getPrototypeOf(a);out.push(['side',a instanceof Duplex,b instanceof Duplex,a!==b,Object.getPrototypeOf(b)===proto,proto.constructor.name,Object.getOwnPropertyNames(proto),Object.getOwnPropertySymbols(proto).map(s=>[String(s),Object.getOwnPropertyDescriptor(proto,s).value.name]),Object.getOwnPropertySymbols(a).map(s=>[String(s),typeof a[s]])]);
const item={id:1};a.write(item,()=>events.push('write-callback'));events.push(['queued',a.writableLength,b.readableLength]);const value=b.read();events.push(['read',value===item]);a.end(()=>events.push('finish-callback'));b.on('end',()=>events.push('peer-end'));b.resume();a.on('finish',()=>events.push('finish'));setImmediate(()=>{out.push(['flow',events]);const [c,d]=f();const error=new Error('marker'),closed=[];c.on('error',e=>closed.push(['error',e===error]));d.on('error',()=>closed.push('peer-error'));c.on('close',()=>closed.push('close'));d.on('close',()=>closed.push('peer-close'));c.destroy(error);setImmediate(()=>{out.push(['destroy',closed,c.destroyed,d.destroyed]);console.log(JSON.stringify(out));});});

"#);
}

#[test]
fn native_stream_emitters_preserve_capture_flags_limits_and_domain_owner() {
compare_node_stream_source(r#"
const {Readable,Writable,Duplex,PassThrough}=require('stream'),EE=require('events'),out=[];
for(const Type of [Readable,Writable,Duplex,PassThrough]){const s=new Type({read(){},write(c,e,cb){cb()},captureRejections:true});const symbols=Object.getOwnPropertySymbols(s),shape=symbols.find(x=>String(x)==='Symbol(shapeMode)'),capture=symbols.find(x=>String(x)==='Symbol(kCapture)');s.setMaxListeners(7);out.push([Type.name,s[shape],s[capture],s._maxListeners,s.getMaxListeners(),Object.getOwnPropertyDescriptor(s,capture)]);s.destroy();}
const domain=require('domain').create(),marker=new Error('marker');let stream;domain.on('error',e=>out.push(['domain-error',e===marker,e.domainEmitter===stream,e.domain===domain]));domain.run(()=>{stream=new PassThrough({autoDestroy:false});out.push(['domain',stream.domain===domain,domain.members.includes(stream),domain.members.length]);});try{stream.emit('error',marker)}catch(e){out.push(['thrown',e===marker])}setImmediate(()=>console.log(JSON.stringify(out)));

"#,
    );
}

#[test]
fn http_transfer_codings_match_node_default_error_order_and_wire_bytes() {
    compare_node_stream_source(
        r#"
const http=require('http'),net=require('net');
(async()=>{const rows=[];for(const [te,early] of [...['chunkedchunked','gzip','chunked, chunked','chunked, gzip','gzip, chunked','CHUNKED'].map(te=>[te,false]),['gzip',true]])await new Promise(resolve=>{
 const events=[];const s=http.createServer((req,res)=>{events.push('request');res.sendDate=false;if(early)res.end('early');req.on('data',b=>events.push('data:'+b));req.on('end',()=>{events.push('end');res.end('ok')})});
 s.listen(0,'127.0.0.1',()=>{let wire='';const c=net.connect(s.address().port,'127.0.0.1',()=>c.write('POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: '+te+'\r\nConnection: close\r\n\r\n1\r\nA\r\n0\r\n\r\n'));c.setEncoding('utf8');c.on('data',b=>wire+=b);c.on('end',()=>s.close(()=>{rows.push([te,early,events,wire.startsWith('HTTP/1.1 400')?wire:[wire.split('\r\n')[0],wire.split('\r\n\r\n')[1]]]);resolve()}));});
});console.log(JSON.stringify(rows))})();
"#,
    );
}

#[test]
fn util_parse_args_preserves_defaults_getter_order_tokens_and_strict_writes() {
    compare_node_stream_source(include_str!("../node-tests/test-util-parse-args-contract.js"));
}

#[test]
fn util_parse_args_uses_captured_intrinsics_after_nonconfigurable_guest_replacement() {
    compare_node_stream_source(r#"
const savedStringify=JSON.stringify;
for(const [owner,key] of [[Array.prototype,'slice'],[Array.prototype,'shift'],[Array.prototype,'unshift'],[Array.prototype,'push'],[Array.prototype,'map'],[Object,'hasOwn'],[Object,'entries'],[Reflect,'apply'],[String.prototype,'charAt'],[String.prototype,'startsWith'],[String.prototype,'includes'],[String.prototype,'indexOf'],[String.prototype,'slice']]){
Object.defineProperty(owner,key,{value(){throw Error('guest intrinsic '+key)},configurable:false,writable:false});
}
const parse=require('node:util').parseArgs;
const result=parse({args:['-abx','--no-flag','--','pos'],options:{alpha:{type:'boolean',short:'a'},beta:{type:'string',short:'b'},flag:{type:'boolean'}},allowNegative:true,allowPositionals:true,tokens:true});
process.stdout.write(savedStringify({name:parse.name,length:parse.length,prototype:Object.hasOwnProperty.call(parse,'prototype'),result})+'\n');
"#);
}

#[test]
fn util_parse_args_default_argv_matches_eval_print_and_file_entry_forms() {
    let directory = tempfile::tempdir().unwrap();
    let source = "JSON.stringify(require('util').parseArgs({strict:false}).values)";
    std::fs::write(directory.path().join("entry.cjs"), format!("process.stdout.write({source})")).unwrap();
    std::fs::write(directory.path().join("preload.cjs"),format!("process.stdout.write({source})")).unwrap();
    for args in [
        vec!["-e".to_owned(),format!("process.stdout.write({source})"),"--".into(),"--foo".into(),"--bar".into()],
        vec!["--eval".to_owned(),format!("process.stdout.write({source})"),"--".into(),"--foo".into(),"--bar".into()],
        vec![format!("--eval=process.stdout.write({source})"),"--".into(),"--foo".into(),"--bar".into()],
        vec!["-p".into(),source.into(),"--".into(),"--foo".into(),"--bar".into()],
        vec!["--print".into(),"--eval".into(),source.into(),"--".into(),"--foo".into(),"--bar".into()],
        vec!["--print".into(),format!("--eval={source}"),"--".into(),"--foo".into(),"--bar".into()],
        vec!["-r".into(),"./preload.cjs".into(),"-e".into(),"".into(),"--".into(),"--foo".into(),"--bar".into()],
        vec!["-r".into(),"./preload.cjs".into(),"-p".into(),"".into(),"--".into(),"--foo".into(),"--bar".into()],
        vec!["entry.cjs".into(),"--foo".into(),"--bar".into()],
        vec!["--".into(),"entry.cjs".into(),"--foo".into(),"--bar".into()],
    ] {
        let observations = ["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.current_dir(directory.path()).args(&args);
            if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1"); }
            quench_node_test::case_process::observe_command(command,Duration::from_secs(10)).unwrap()
        });
        for observation in &observations {
            assert!(!observation.timed_out,"{args:?}");
            assert_eq!(observation.exit_code,Some(0),"{args:?}: {}",String::from_utf8_lossy(&observation.stderr));
            assert!(observation.stderr.is_empty(),"{args:?}");
        }
        assert_eq!(observations[0].stdout,observations[1].stdout,"{args:?}");
    }
}

#[test]
fn native_builtin_setup_preserves_regexp_and_crypto_uses_canonical_streams() {
    compare_node_stream_source(r#"
const state=()=>[RegExp.input,RegExp.lastMatch,RegExp.lastParen,RegExp.leftContext,RegExp.rightContext,RegExp.$1,RegExp.$2,RegExp.$9];
/(before)-(after)/.exec('leftbefore-afterright');RegExp.input='caller input';const before=JSON.stringify(state());
const crypto=require('crypto');const after=JSON.stringify(state());
const stream=require('stream');const promises=require('stream/promises');
console.log(before===after,stream.promises===promises,stream.Stream===stream);
const hash=crypto.createHash('sha256');console.log(hash instanceof stream.Transform);
hash.on('data',chunk=>console.log(chunk.toString('hex')));hash.end('abc');
"#);
}

#[test]
fn native_internal_microtask_queue_keeps_original_identity_and_job_order() {
    const SOURCE: &str = r#"
const original=queueMicrotask;
Object.defineProperty(globalThis,'queueMicrotask',{value(){throw Error('public queue consulted')},writable:false,configurable:false});
const internal=require('internal/process/task_queues');
console.log(internal.queueMicrotask===original,internal.queueMicrotask.name,internal.queueMicrotask.length);
const trace=[];process.nextTick(()=>trace.push('tick'));
internal.queueMicrotask(()=>{trace.push('microtask');console.log(trace.join(','))});trace.push('sync');
"#;
    compare_node_source_with_args(SOURCE, &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_promise_inspection_preserves_slots_identity_and_rejection_handling() {
    const SOURCE: &str = r#"
const {internalBinding}=require('internal/test/binding');const u=internalBinding('util');
const value={answer:42},pending=new Promise(()=>{}),fulfilled=Promise.resolve(value),rejected=Promise.reject(value);
for(const p of [pending,fulfilled,rejected])for(const key of ['then','constructor'])Object.defineProperty(p,key,{get(){throw Error(key)}});
console.log(u.getPromiseDetails(pending).length,u.getPromiseDetails(pending)[0],u.getPromiseDetails(fulfilled)[0],u.getPromiseDetails(rejected)[0]);
console.log(u.getPromiseDetails(fulfilled)[1]===value,u.getPromiseDetails(rejected)[1]===value,u.getPromiseDetails({}));
u.markPromiseAsHandled(rejected);u.markPromiseAsHandled(pending);
const events=[];process.on('unhandledRejection',()=>events.push('unhandled'));process.on('rejectionHandled',()=>events.push('handled'));
setImmediate(()=>console.log(JSON.stringify(events)));
"#;
    compare_node_source_with_args(SOURCE, &["--no-warnings", "--expose-internals"]);
}

#[test]
fn canonical_web_streams_share_globals_and_preserve_byte_and_abort_algorithms() {
    compare_node_stream_source(r#"
const web=require('stream/web');
console.log(['ReadableStream','WritableStream','TransformStream','TextEncoderStream','TextDecoderStream'].every(name=>web[name]===globalThis[name]));
(async()=>{
 const bytes=new web.ReadableStream({type:'bytes',start(c){c.enqueue(new Uint8Array([1,2,3]));c.close()}});
 const reader=bytes.getReader({mode:'byob'});const buffer=new Uint8Array(8);const first=await reader.read(buffer);
 console.log(first.done,JSON.stringify([...first.value]),buffer.byteLength);
 const final=await reader.read(new Uint8Array(2));console.log(final.done,final.value.byteLength);
 const reason={reason:42};await new web.ReadableStream({cancel(value){console.log('cancel',value===reason)}}).cancel(reason);
 const writable=new web.WritableStream({abort(value){console.log('abort',value===reason)}});await writable.abort(reason);
 const encoder=new web.TextEncoderStream(),r=encoder.readable.getReader(),w=encoder.writable.getWriter();
 const read=r.read();await w.write('\ud83d');await w.write('\ude00');console.log('encoded',JSON.stringify([...(await read).value]));
 await w.close();console.log('closed',(await r.read()).done);
})().catch(error=>{console.error(error);process.exitCode=1});
"#);
}

#[test]
fn native_encoding_and_array_buffer_copy_match_stable_slots_and_byte_ranges() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-encoding-bytes.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn structured_clone_views_preserve_backing_aliases_transfers_and_resize_modes() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),out=[];
for(const Type of [Uint8Array,Uint8ClampedArray,Uint16Array,Uint32Array,Int8Array,Int16Array,Int32Array,BigInt64Array,BigUint64Array,Float16Array,Float32Array,Float64Array]){
 const buffer=new ArrayBuffer(64),bytes=new Uint8Array(buffer);for(let i=0;i<64;i++)bytes[i]=i;
 const view=new Type(buffer,16,2),data=new DataView(buffer,8,24);Object.defineProperty(view,'buffer',{get(){throw 'buffer hook'}});Object.defineProperty(view,'extra',{enumerable:true,get(){throw 'extra'}});
 const [copy,v,d,alias]=structuredClone([buffer,view,data,view]);assert.notStrictEqual(copy,buffer);assert.strictEqual(v.buffer,copy);assert.strictEqual(d.buffer,copy);assert.strictEqual(alias,v);assert.strictEqual(Object.getPrototypeOf(v),Type.prototype);assert.strictEqual(v.byteOffset,16);assert.strictEqual(v.length,2);assert.strictEqual(d.byteOffset,8);assert.strictEqual(d.byteLength,24);assert.deepStrictEqual([...new Uint8Array(copy)],[...bytes]);new Uint8Array(copy)[0]=255;assert.strictEqual(bytes[0],0);out.push([Type.name,v.byteOffset,v.length]);
}
for(const transfer of [false,true]){const b=new ArrayBuffer(16,{maxByteLength:32}),fixed=new Uint16Array(b,4,2),tracking=new Uint16Array(b,4),dataFixed=new DataView(b,4,4),dataTracking=new DataView(b,4);const [c,f,t,df,dt]=structuredClone([b,fixed,tracking,dataFixed,dataTracking],transfer?{transfer:[b]}:{});assert.strictEqual(b.byteLength,transfer?0:16);assert.strictEqual(c.resizable,true);assert.strictEqual(c.maxByteLength,32);for(const view of [f,t,df,dt])assert.strictEqual(view.buffer,c);c.resize(24);assert.strictEqual(f.length,2);assert.strictEqual(t.length,10);assert.strictEqual(df.byteLength,4);assert.strictEqual(dt.byteLength,20);}
for(const kind of ['typed','data','buffer']){const b=new ArrayBuffer(8),v=kind==='typed'?new Uint16Array(b):kind==='data'?new DataView(b):b;structuredClone(b,{transfer:[b]});assert.throws(()=>structuredClone(v),e=>e.name==='DataCloneError'&&e.code===25&&e.message==='An ArrayBuffer is detached and could not be cloned.');}
for(const kind of ['typed','data']){const b=new ArrayBuffer(8,{maxByteLength:16}),v=kind==='typed'?new Uint16Array(b,4,2):new DataView(b,4,4);b.resize(2);assert.throws(()=>structuredClone(v),e=>e.name==='DataCloneError'&&e.code===25&&e.message===(kind==='typed'?'[object Object] could not be cloned.':'#<DataView> could not be cloned.'));}
const b=new ArrayBuffer(8),view=new Uint8Array(b);assert.throws(()=>structuredClone({view,bad:()=>{}},{transfer:[b]}),e=>e.name==='DataCloneError');assert.strictEqual(b.byteLength,8);
const original=Uint16Array;globalThis.Uint16Array=function(){throw 'replacement'};const copy=structuredClone(new original([42]));assert.strictEqual(Object.getPrototypeOf(copy),original.prototype);globalThis.Uint16Array=original;
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn structured_clone_boxed_values_and_rejected_brands_match_node() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),out=[];
for(const value of [true,false,0,-0,NaN,Infinity,42n,'a\ud800b']) {
 const source=Object(value),prototype=Object.getPrototypeOf(source);
 Object.defineProperty(source,'valueOf',{get(){throw 'valueOf hook'}});Object.defineProperty(source,Symbol.toPrimitive,{get(){throw 'primitive hook'}});Object.defineProperty(source,'extra',{enumerable:true,get(){throw 'extra hook'}});
 const [copy,alias]=structuredClone([source,source]);assert.strictEqual(copy,alias);assert.notStrictEqual(copy,source);assert.strictEqual(Object.getPrototypeOf(copy),prototype);assert(Object.is(prototype.valueOf.call(copy),value));assert.strictEqual(Object.hasOwn(copy,'extra'),false);out.push([typeof value,Object.keys(copy)]);
}
for(const Type of [Boolean,Number,String]){const copy=structuredClone(Type.prototype);assert.strictEqual(Object.getPrototypeOf(copy),Type.prototype);assert(Object.is(copy.valueOf(),Type.prototype.valueOf()));}
for(const [value,message] of [[Object(Symbol('x')),'[object Symbol] could not be cloned.'],[Promise.resolve(),'#<Promise> could not be cloned.'],[(function(){return arguments})(1),'#<Object> could not be cloned.']]){Object.defineProperty(value,'extra',{enumerable:true,get(){throw 'rejected getter'}});assert.throws(()=>structuredClone(value),e=>e.name==='DataCloneError'&&e.code===25&&e.message===message);}
const source=Object(12n),original=BigInt;globalThis.BigInt=function(){throw 'replacement'};const copy=structuredClone(source);assert.strictEqual(Object.getPrototypeOf(copy),original.prototype);globalThis.BigInt=original;
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn structured_clone_webidl_options_preserve_errors_iteration_and_original_intrinsics() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),out=[];
try{structuredClone()}catch(e){out.push([e.name,e.code,e.message]);}
for(const options of ['',1,true,1n,Symbol('x'),{transfer:null},{transfer:''},{transfer:1},{transfer:[1]},{transfer:[null]},{transfer:{[Symbol.iterator]:1}}]){
 try{structuredClone(undefined,options);throw 'accepted invalid options'}catch(e){assert.strictEqual(e.code,'ERR_INVALID_ARG_TYPE');out.push([e.name,e.code,e.message]);}
}
assert.strictEqual(structuredClone(undefined,null),undefined);assert.strictEqual(structuredClone(undefined,{transfer:undefined}),undefined);
const trace=[],buffer=new ArrayBuffer(8),options={get transfer(){trace.push('transfer');return {get [Symbol.iterator](){trace.push('iterator');return function(){trace.push('call');let done=false;return {get next(){trace.push('next');return function(){trace.push('step');if(done)return {done:true};done=true;return {get done(){trace.push('done');return false},get value(){trace.push('value');return buffer}}}}}}}}}};
const copy=structuredClone(buffer,options);assert.strictEqual(copy.byteLength,8);assert.strictEqual(buffer.byteLength,0);out.push(trace);
const marker={};assert.throws(()=>structuredClone({}, {get transfer(){throw marker}}),e=>e===marker);
const original=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function(){throw 'public array iterator'};const b=new ArrayBuffer(1);const cloned=structuredClone(b,{transfer:{[Symbol.iterator](){let done=false;return {next(){if(done)return {done:true};done=true;return {value:b,done:false}}}}}});Array.prototype[Symbol.iterator]=original;assert.strictEqual(cloned.byteLength,1);assert.strictEqual(b.byteLength,0);
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn filesystem_relative_paths_follow_chdir_without_repository_aliases() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert');
const original=process.cwd(),dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-relative-paths-')),out=[];
try {
 for(const relative of ['test/common/index.js','tests/node/test/common/index.js']){const file=path.join(dir,relative);fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,'application-owned:'+relative);}
 process.chdir(dir);
 for(const relative of ['test/common/index.js','tests/node/test/common/index.js']){const content='application-owned:'+relative;assert.strictEqual(fs.readFileSync(relative,'utf8'),content);assert.strictEqual(fs.statSync(relative).size,Buffer.byteLength(content));out.push([relative,fs.readFileSync(relative,'utf8')]);}
} finally {process.chdir(original);fs.rmSync(dir,{recursive:true,force:true});}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn structured_clone_transfer_markers_and_commit_order_match_node() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),w=require('node:worker_threads'),out=[];
for(const name of ['markAsUncloneable','markAsUntransferable','isMarkedAsUntransferable']){const f=w[name],d=Object.getOwnPropertyDescriptor(f,'prototype');out.push([f.name,f.length,Object.hasOwn(f,'prototype'),d.writable,d.enumerable,d.configurable,f.prototype.constructor===f]);}
for(const primitive of [undefined,null,1,1n,'x',Symbol('x')]){assert.strictEqual(w.markAsUntransferable(primitive),undefined);assert.strictEqual(w.isMarkedAsUntransferable(primitive),false);}
for(const value of [{},[],new Map(),new Set(),new Date(),new Error(),new Uint8Array(2),new ArrayBuffer(2),function(){}]){assert.strictEqual(w.isMarkedAsUntransferable(value),false);assert.strictEqual(w.markAsUntransferable(value),undefined);assert.strictEqual(w.isMarkedAsUntransferable(value),true);w.markAsUntransferable(value);assert.throws(()=>structuredClone(1,{transfer:[value]}),e=>e.name==='DataCloneError'&&e.code===25&&e.message==='Cannot transfer object of unsupported type.');}
for(const revoked of [false,true]){const trace=[],{proxy,revoke}=Proxy.revocable({},{get(){trace.push('get')},set(){trace.push('set');return true}});if(revoked)revoke();assert.strictEqual(w.isMarkedAsUntransferable(proxy),false);for(const name of ['markAsUntransferable','markAsUncloneable'])assert.throws(()=>w[name](proxy),e=>e.name==='TypeError'&&e.message==='Cannot pass private property name to proxy trap');assert.deepStrictEqual(trace,[]);}
const marked=new ArrayBuffer(2),keys=Reflect.ownKeys(marked);w.markAsUntransferable(marked);assert.deepStrictEqual(Reflect.ownKeys(marked),keys);const unmarkedCopy=structuredClone(marked);assert.strictEqual(w.isMarkedAsUntransferable(unmarkedCopy),false);for(const method of ['transfer','transferToFixedLength'])assert.throws(()=>marked[method](),e=>e.name==='TypeError'&&e.message==='ArrayBuffer is not detachable and could not be cloned.');assert.strictEqual(marked.byteLength,2);
function error(operation){try{operation();return null}catch(e){return [e.name,e.code??null,e.message]}}
const b=new ArrayBuffer(2),detached=new ArrayBuffer(2);detached.transfer();out.push(error(()=>structuredClone(1,{transfer:[b,b]})));out.push(error(()=>structuredClone(1,{transfer:[{}]})));out.push(error(()=>structuredClone(1,{transfer:[detached]})));out.push(error(()=>structuredClone(1,{transfer:[new WebAssembly.Memory({initial:1}).buffer]})));
for(const action of ['resize','detach','write']){const b=new ArrayBuffer(4,{maxByteLength:16});const c=structuredClone({b,get x(){if(action==='resize')b.resize(8);else if(action==='detach')b.transfer();else new Uint8Array(b)[0]=7;return 1}},{transfer:[b]});out.push([action,c.b.byteLength,c.b.resizable,c.b.maxByteLength,Array.from(new Uint8Array(c.b))]);assert.strictEqual(b.byteLength,0);}
for(const first of [true,false]){const b=new ArrayBuffer(8,{maxByteLength:16}),v=new Uint8Array(b,4,4);const source=first?{v,get x(){b.resize(2);return 1}}:{get x(){b.resize(2);return 1},v};const result=error(()=>structuredClone(source,{transfer:[b]}));out.push(['view-order',first,result,b.byteLength]);}
{const b=new ArrayBuffer(4,{maxByteLength:16});const result=error(()=>structuredClone({b,get v(){b.resize(8);return new Uint8Array(b,4,4)}}));out.push(['copied-view',result,b.byteLength]);}
{const a=new ArrayBuffer(2),b=new ArrayBuffer(2);const result=error(()=>structuredClone({get x(){w.markAsUntransferable(b);return 1}},{transfer:[a,b]}));out.push(['detach-key',result,a.byteLength,b.byteLength]);assert.strictEqual(a.byteLength,0);assert.strictEqual(b.byteLength,2);}
{const b=new ArrayBuffer(2),marker={};assert.throws(()=>structuredClone({b,get x(){throw marker}},{transfer:[b]}),e=>e===marker);assert.strictEqual(b.byteLength,2);}
for(const transfer of [false,true])for(const rab of [false,true])for(const type of ['typed','data']){const b=rab?new ArrayBuffer(8,{maxByteLength:16}):new ArrayBuffer(8),v=type==='typed'?new Uint8Array(b,0,2):new DataView(b,0,2);let copy;const failure=error(()=>{copy=structuredClone({b,get x(){b.transfer();return 1},v},transfer?{transfer:[b]}:{})});out.push(['detached-view',transfer,rab,type,failure,copy?[copy.v.byteOffset,copy.v.byteLength,copy.b.byteLength]:null]);}
for(const type of ['typed','data']){const b=new ArrayBuffer(4,{maxByteLength:16}),v=type==='typed'?new Uint8Array(b):new DataView(b);const c=structuredClone({b,get x(){b.resize(8);return 1},v});assert.strictEqual(c.v.byteLength,4);assert.strictEqual(c.b.byteLength,4);}

console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn filesystem_metadata_file_urls_validate_scheme_host_and_encoded_separators() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),assert=require('node:assert'),out=[];
const cases=[['https://example.org/','ERR_INVALID_URL_SCHEME'],['file://example.org/','ERR_INVALID_FILE_URL_HOST'],['file:///a%2Fb','ERR_INVALID_FILE_URL_PATH']];
for(const [input,code] of cases){
 const path=new URL(input);assert.strictEqual(fs.existsSync(path),false);
 for(const name of ['statSync','lstatSync','statfsSync']){assert.throws(()=>fs[name](path),e=>{out.push([name,e.name,e.code,e.message]);return e.code===code});}
 for(const name of ['stat','lstat','statfs']){let called=false;assert.throws(()=>fs[name](path,()=>{called=true}),e=>{out.push([name,e.name,e.code,e.message]);return e.code===code});assert.strictEqual(called,false);}
}
for(const name of ['statSync','lstatSync'])assert.strictEqual(fs[name](new URL('file:///')).isDirectory(),true);
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn filesystem_metadata_callback_exceptions_escape_without_second_delivery() {
    for method in ["stat", "lstat", "statfs"] {
        for missing in [false, true] {
            let source = format!(
                r#"
const fs=require('node:fs');
const marker={{}},method={method:?};let count=0;
process.on('uncaughtException',error=>console.log('uncaught',error===marker,count));
fs[method]({path},function(error,stats){{count++;console.log('callback',count,error===null,!!stats);throw marker;}});
"#,
                path = if missing {
                    "'/quench-metadata-missing-'+process.pid"
                } else {
                    "'/'"
                },
            );
            compare_node_stream_source(&source);
        }
    }
}

#[test]
fn file_url_conversion_observes_url_like_getters_options_and_primordials() {
    compare_node_stream_source(
        r#"
const {fileURLToPath}=require('node:url'),assert=require('node:assert'),fs=require('node:fs'),out=[];
function result(f){try{return ['value',f()]}catch(e){return ['error',e.name,e.code??null,e.message]}}
for(const input of [undefined,null,1,true,{},[],Symbol('x')])out.push(result(()=>fileURLToPath(input)));
for(const input of ['file:///tmp/a%20b','file://localhost/tmp/a','file://host/tmp','https://example.org/','file:///a%2fb','file:///a%5cb','file:///%FF','file:///%ZZ','file:///C:/foo','file://xn--bcher-kva/share'])for(const windows of [false,true,null])out.push([input,windows,result(()=>fileURLToPath(input,{windows}))]);
for(const input of [new URL('file:///a%2Fb'),{href:'file:///',protocol:'file:',hostname:'',pathname:'/a%2Fb'}])assert.throws(()=>fileURLToPath(input),e=>{assert.strictEqual(e.input,input);const d=Object.getOwnPropertyDescriptor(e,'input');assert.deepStrictEqual([d.writable,d.enumerable,d.configurable],[true,true,true]);out.push([String(e),Reflect.ownKeys(e).filter(k=>k!=='stack')]);return true});
const trace=[],u={};for(const [name,value] of Object.entries({href:'file:///',protocol:'file:',auth:undefined,path:undefined,hostname:'',pathname:'/tmp/a%20b'}))Object.defineProperty(u,name,{get(){trace.push(name);return value}});
const options={get windows(){trace.push('windows');return false}};assert.strictEqual(fileURLToPath(u,options),'/tmp/a b');out.push(['order',trace]);
const marker={};assert.throws(()=>fileURLToPath(null,{get windows(){throw marker}}),e=>e===marker);
for(const name of ['href','protocol','auth','path','hostname','pathname']){const u=new URL('file:///');Object.defineProperty(u,name,{get(){throw marker}});assert.throws(()=>fileURLToPath(u),e=>e===marker);assert.throws(()=>fs.statSync(u),e=>e===marker);}
{const pathname={length:0,toString(){return '/object-path'}},u={href:'file:///',protocol:'file:',hostname:'',pathname};assert.strictEqual(fileURLToPath(u),pathname);}
{const pathname='\ud800',u={href:'file:///',protocol:'file:',hostname:'',pathname};assert.strictEqual(fileURLToPath(u),pathname);}
const originalURL=globalThis.URL,originalIncludes=String.prototype.includes,originalDecode=globalThis.decodeURIComponent;
try{globalThis.URL=function(){throw marker};String.prototype.includes=function(){throw marker};globalThis.decodeURIComponent=function(){throw marker};assert.strictEqual(fileURLToPath('file:///tmp/a%20b'),'/tmp/a b');}finally{globalThis.URL=originalURL;String.prototype.includes=originalIncludes;globalThis.decodeURIComponent=originalDecode;}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn uri_globals_preserve_node_error_messages_and_utf16() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),out=[];
for(const name of ['encodeURI','encodeURIComponent'])for(const value of ['\ud800','\udfff','a\ud800b'])assert.throws(()=>globalThis[name](value),e=>{out.push([name,e.name,e.message,e.code??null]);return e instanceof URIError&&e.message==='URI malformed'});
for(const name of ['decodeURI','decodeURIComponent'])for(const value of ['%','%0','%ZZ','%FF','%C0%80','%ED%A0%80','%F4%90%80%80','%E2%82','%E2x%82%AC'])assert.throws(()=>globalThis[name](value),e=>{out.push([name,e.name,e.message,e.code??null]);return e instanceof URIError&&e.message==='URI malformed'});
assert.strictEqual(decodeURIComponent('\ud800'),'\ud800');assert.strictEqual(decodeURIComponent('%F0%90%80%80'),'𐀀');assert.strictEqual(decodeURI('%2f'),'%2f');assert.strictEqual(decodeURIComponent('%2f'),'/');
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn file_url_byte_conversion_preserves_raw_paths_forgiving_escapes_and_hooks() {
    compare_node_stream_source(
        r#"
const url=require('node:url'),assert=require('node:assert'),B=Buffer,out=[];
function result(f){try{const b=f();assert(B.isBuffer(b));return ['bytes',Array.from(b)]}catch(e){return ['error',e.name,e.code??null,e.message]}}
for(const path of ['file:///tmp/a%20b','file:///%82%A0%82%A2','file:///a%2Fb','file:///a%5Cb','file:///%ZZ%2%','file:///C:/%FF%2F%5C','file://xn--bcher-kva/share%FF','https://example.org/'])for(const windows of [false,true,null])out.push([path,windows,result(()=>url.fileURLToPathBuffer(path,{windows}))]);
const trace=[],u={};for(const [name,value] of Object.entries({href:'file:///',protocol:'file:',auth:undefined,path:undefined,hostname:'',pathname:'/a%FF%2Fb'}))Object.defineProperty(u,name,{get(){trace.push(name);return value}});out.push(result(()=>url.fileURLToPathBuffer(u,{get windows(){trace.push('windows');return false}})),trace);
for(const path of ['https://example.org/','file://remote/']){try{url.fileURLToPath(path)}catch(e){out.push([String(e),Reflect.ownKeys(e).filter(k=>k!=='stack')])}}
const marker={};assert.throws(()=>url.fileURLToPathBuffer(null,{get windows(){throw marker}}),e=>e===marker);
for(const name of ['href','protocol','auth','path','hostname','pathname']){const u=new URL('file:///');Object.defineProperty(u,name,{get(){throw marker}});assert.throws(()=>url.fileURLToPathBuffer(u),e=>e===marker);}
const originalFrom=Buffer.from,originalGlobal=globalThis.Buffer,calls=[];
try{Buffer.from=function(...args){calls.push([typeof args[0],args[0] instanceof ArrayBuffer,args.length,args[1]??null,args[2]??null]);return Reflect.apply(originalFrom,this,args)};globalThis.Buffer=function(){throw marker};out.push(result(()=>url.fileURLToPathBuffer('file:///tmp/a%FF',{windows:false})));}finally{globalThis.Buffer=originalGlobal;Buffer.from=originalFrom;}
out.push(calls);console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn filesystem_metadata_paths_preserve_bytes_validation_and_option_getter_order() {
    use std::os::unix::ffi::OsStringExt;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory
            .path()
            .join(std::ffi::OsString::from_vec(vec![0xff])),
        b"raw",
    )
    .unwrap();
    std::fs::write(directory.path().join("x"), b"xxx").unwrap();
    std::fs::write(directory.path().join("y"), b"yyyyyyy").unwrap();
    let source = format!(
        "const directory={:?};\n",
        directory.path().to_str().unwrap()
    ) + r#"
const fs=require('node:fs'),assert=require('node:assert'),out=[];
for(const detached of [false,true]){const b=new ArrayBuffer(8,{maxByteLength:16}),v=new Uint8Array(b,4,2);if(detached)b.transfer();else b.resize(2);for(const method of ['statSync','lstatSync','statfsSync']){try{fs[method](v);assert.fail('accepted unavailable backing bytes')}catch(e){assert(e instanceof TypeError);out.push([method,detached,e.name,e.message,e.code??null]);}}}
const raw=Buffer.concat([Buffer.from(directory+'/'),Buffer.from([255])]);
const rawLength=raw.length;raw.toString=raw.includes=()=>{throw 'public-conversion'};for(const key of ['length','byteLength','byteOffset','buffer'])Object.defineProperty(raw,key,{get(){throw 'public-view-metadata'}});
for(const method of ['statSync','lstatSync','statfsSync']){
 const b=new Uint8Array(rawLength+2);b.set(raw,1);const view=b.subarray(1,-1);
 for(const path of [raw,view]){const value=fs[method](path);assert(value);if(method!=='statfsSync')assert.strictEqual(value.size,3);out.push([method,typeof value.size,typeof value.bsize]);}
 for(const path of ['a\0b',Buffer.from([97,0,98]),new Uint8Array([97,0,98]),new Uint8ClampedArray([47]),new Int8Array([47]),new DataView(new ArrayBuffer(1)),{},null,undefined]){
  try{fs[method](path);assert.fail('accepted invalid path')}catch(e){assert(e instanceof TypeError);out.push([method,e.name,e.code,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack')]);}
 }
 try{fs[method]('')}catch(e){out.push([method,e.code,e.syscall,e.path,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack')]);}
 const path=Buffer.from(directory+'/x'),trace=[];
 const options={get bigint(){trace.push('bigint');path[path.length-1]=121;return false},get throwIfNoEntry(){trace.push('throwIfNoEntry');return true}};
 const stats=fs[method](path,options);if(method!=='statfsSync')assert.strictEqual(stats.size,7);out.push([method,trace]);
}
for(const method of ['statSync','lstatSync'])for(const path of ['',directory+'/missing',directory+'/x/missing']){
 try{out.push([method,path.endsWith('/x/missing')?'not-directory':path?'missing':'empty',fs[method](path,{throwIfNoEntry:false})===undefined])}catch(e){out.push([method,'not-directory',e.code,e.syscall]);}
}
console.log(JSON.stringify(out));
"#;
    compare_node_stream_source(&source);
}

#[cfg(target_os = "linux")]
#[test]
fn filesystem_metadata_relative_paths_follow_directory_identity_after_rename_and_unlink() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert');
const original=process.cwd(),base=fs.mkdtempSync(path.join(os.tmpdir(),'quench-metadata-cwd-')),old=path.join(base,'old'),renamed=path.join(base,'renamed');
try{fs.mkdirSync(old);fs.writeFileSync(path.join(old,'x'),'data');process.chdir(old);fs.renameSync(old,renamed);assert.strictEqual(fs.statSync('x').size,4);assert.strictEqual(fs.lstatSync('x').size,4);assert(fs.statfsSync('.'));fs.unlinkSync(path.join(renamed,'x'));fs.rmdirSync(renamed);assert(fs.statSync('.').isDirectory());assert(fs.lstatSync('.').isDirectory());assert(fs.statfsSync('.'));console.log('renamed and unlinked directory identity');}finally{process.chdir(original);fs.rmSync(base,{recursive:true,force:true});}
"#,
    );
}

#[test]
fn typed_array_base_method_receiver_failures_match_node() {
    compare_node_stream_source(
        r#"
const out=[],assert=require('node:assert');
for(const name of ['includes','indexOf','join','reverse','fill','copyWithin','slice','keys','values','entries']){
 for(const kind of ['brand','detached','out-of-bounds']){
  let value={toString(){throw Error('public conversion')}};
  if(kind!=='brand'){const b=new ArrayBuffer(8,{maxByteLength:16});value=new Uint8Array(b,4,2);if(kind==='detached')b.transfer();else b.resize(2);}
  let called=false;const arg={valueOf(){called=true;throw Error('converted argument')},toString(){called=true;throw Error('converted argument')}};
  assert.throws(()=>Uint8Array.prototype[name].call(value,arg),e=>{out.push([name,kind,e.name,e.message,e.code??null]);return e instanceof TypeError});assert.strictEqual(called,false);
 }
}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn filesystem_statfs_results_share_node_constructor_prototypes_and_field_descriptors() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),assert=require('node:assert'),out=[];
const numeric=fs.statfsSync('/'),big=fs.statfsSync('/',{bigint:true}),C=numeric.constructor;
assert.strictEqual(C,big.constructor);assert.strictEqual(C.name,'StatFs');assert.strictEqual(C.length,8);
assert.strictEqual(Object.getPrototypeOf(numeric),C.prototype);assert.strictEqual(Object.getPrototypeOf(big),C.prototype);
for(const value of [numeric,big]){out.push([value.constructor.name,Reflect.ownKeys(Object.getPrototypeOf(value)),Object.entries(Object.getOwnPropertyDescriptors(value)).map(([k,d])=>[k,typeof d.value,d.writable,d.enumerable,d.configurable])]);}
const made=new C(1,2,3,4,5,6,7,8);out.push(Object.entries(made));assert.throws(()=>C(),TypeError);
C.prototype.marker='shared-statfs';assert.strictEqual(fs.statfsSync('/').marker,'shared-statfs');delete C.prototype.marker;
const marker={},seen=[];Object.defineProperty(C.prototype,'blocks',{configurable:true,set(value){seen.push(typeof value);throw marker}});for(const bigint of [false,true])assert.throws(()=>fs.statfsSync('/',{bigint}),e=>e===marker);delete C.prototype.blocks;out.push(seen);
const callback=new Promise(resolve=>fs.statfs('/',(error,value)=>{assert.strictEqual(error,null);assert.strictEqual(value.constructor,C);resolve(['callback',value.constructor.name]);}));
const promise=fs.promises.statfs('/',{bigint:true}).then(value=>{assert.strictEqual(value.constructor,C);assert.strictEqual(typeof value.blocks,'bigint');return ['promise',value.constructor.name]});
Promise.all([callback,promise]).then(results=>{out.push(...results);console.log(JSON.stringify(out));});
"#,
    );
}

#[test]
fn canonical_string_decoder_preserves_native_boundaries_and_live_state_views() {
    compare_node_stream_source(include_str!("../node-tests/test-string-decoder-native-state.js"));
}

#[test]
fn internal_fast_buffer_shares_public_buffer_identity_and_backing() {
    compare_node_source_with_args(r#"
const {FastBuffer}=require('internal/buffer'),stream=require('stream');
const original=Buffer.from([1,2,3,4]),view=new FastBuffer(original.buffer,original.byteOffset,original.byteLength);
console.log(FastBuffer===Buffer[Symbol.species],FastBuffer.prototype===Buffer.prototype,view instanceof Buffer,view.constructor===Buffer,FastBuffer.name,FastBuffer.length);
view[1]=9;console.log(original.toString('hex'),view.readUInt16BE(0));
const raw=new Uint8Array([65,66]),converted=stream._uint8ArrayToBuffer(raw);
console.log(converted instanceof Buffer,Object.getPrototypeOf(converted)===Buffer.prototype,converted.toString(),converted.buffer===raw.buffer);converted[0]=67;console.log(raw[0]);
"#, &["--no-warnings","--expose-internals"]);
}

#[test]
fn socket_end_counts_final_queued_bytes_and_finishes_without_a_transport() {
    compare_node_stream_source(r#"
const net=require('net'),events=[];const fresh=new net.Socket();fresh.end(()=>events.push(['fresh',fresh.writable]));
const server=net.createServer(socket=>{let body='';socket.setEncoding('utf8');socket.on('data',chunk=>body+=chunk);socket.on('end',()=>{events.push(['received',body]);socket.end()})});
server.listen(0,'127.0.0.1',()=>{
 const socket=new net.Socket({highWaterMark:0});socket.connect(server.address().port,'127.0.0.1');socket.cork();
 socket.write('hé',()=>events.push(['write',socket.bytesWritten]));socket.end('😀',()=>events.push(['finish',socket.bytesWritten,socket.writable]));events.push(['queued',socket.bytesWritten]);
 socket.on('close',()=>server.close(()=>console.log(JSON.stringify(events))));
});
"#);
}

#[test]
fn canonical_assertions_validate_rejections_and_share_success_results() {
    compare_node_stream_source(include_str!("../node-tests/test-canonical-assertions.js"));
}

#[test]
fn native_key_slots_preserve_material_comparison_and_reject_spoofed_brands() {
    compare_node_stream_source(include_str!("../node-tests/test-native-key-slots.js"));
}

#[cfg(unix)]
#[test]
fn read_file_preserves_raw_paths_descriptor_positions_and_utf8_conversion_boundaries() {
    use std::os::unix::ffi::OsStringExt;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory
            .path()
            .join(std::ffi::OsString::from_vec(vec![0xff])),
        b"raw-\xff-text",
    )
    .unwrap();
    std::fs::write(directory.path().join("plain"), b"plain-data").unwrap();
    let source = format!(
        "const directory={:?};\n",
        directory.path().to_str().unwrap()
    ) + r#"
const fs=require('node:fs'),assert=require('node:assert'),out=[];
const raw=Buffer.concat([Buffer.from(directory+'/'),Buffer.from([255])]),n=raw.length;
const pathMarker={};raw.toString=()=>{throw pathMarker};for(const encoding of [undefined,'utf8'])assert.throws(()=>fs.readFileSync(raw,encoding),e=>e===pathMarker);delete raw.toString;
for(const path of [raw,(()=>{const b=new Uint8Array(n+2);b.set(raw,1);return b.subarray(1,-1)})()]){out.push(Array.from(fs.readFileSync(path)),fs.readFileSync(path,'utf8'));}
for(const encoding of ['',null,'buffer']){try{const value=fs.readFileSync(directory+'/plain',encoding);out.push([encoding,Buffer.isBuffer(value),typeof value])}catch(e){out.push([encoding,e.name,e.code,e.message])}}
const original=Buffer.prototype.toString;
try{Buffer.prototype.toString=function(encoding){return 'public-buffer-hook:'+encoding};out.push(fs.readFileSync(raw,'utf8'),fs.readFileSync(raw,'utf-8'),fs.readFileSync(raw,'UTF8'));}finally{Buffer.prototype.toString=original;}
const fd=fs.openSync(directory+'/plain','r');try{const b=Buffer.alloc(1);assert.strictEqual(fs.readSync(fd,b,0,1,null),1);out.push(fs.readFileSync(fd,'utf8'),Array.from(fs.readFileSync(fd)),fs.fstatSync(fd).size);assert.strictEqual(fs.readSync(fd,b,0,1,0),1);assert.strictEqual(b[0],112);}finally{fs.closeSync(fd);}
for(const input of [-1,fd,2147483648,1.5])for(const encoding of [undefined,'utf8']){try{fs.readFileSync(input,encoding);assert.fail('accepted invalid input')}catch(e){out.push([input===fd?'closed':input,encoding??null,e.name,e.code,e.syscall??null,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack')]);}}
for(const path of ['a\0b',Buffer.from([97,0,98]),new Uint8ClampedArray([47]),'']){try{fs.readFileSync(path)}catch(e){out.push([e.name,e.code,e.syscall??null,e.path??null,e.message]);}}
console.log(JSON.stringify(out));
"#;
    compare_node_stream_source(&source);
}

#[test]
fn application_fs_utility_names_do_not_adopt_node_library_types_or_state() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert'),out=[];
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-fs-library-identity-'));
try{
 let C;
 for(const phase of ['before','after']){
  const root=path.join(dir,phase,'internal/fs');fs.mkdirSync(root,{recursive:true});const file=path.join(root,'utils.js');
  fs.writeFileSync(file,`module.exports={applicationValue:17,get BigIntStats(){throw Error('application getter')},get getStatFsFromBinding(){throw Error('application getter')},get vfsState(){throw Error('application getter')}};`);
  const value=require(file);assert.strictEqual(value.applicationValue,17);
  const stats=fs.statSync(dir,{bigint:true}),statfs=fs.statfsSync('/');assert.strictEqual(typeof stats.size,'bigint');assert.strictEqual(statfs.constructor.name,'StatFs');
  if(C)assert.strictEqual(statfs.constructor,C);C=statfs.constructor;out.push([phase,value.applicationValue,typeof stats.size,C.name]);
 }
}finally{fs.rmSync(dir,{recursive:true,force:true});}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn read_file_options_flags_and_getter_order_match_node_policy() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert'),out=[];
const original=process.cwd(),dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-options-'));
function capture(options,file='plain'){fs.writeFileSync('plain','data');try{const value=fs.readFileSync(file,options);return ['value',Buffer.isBuffer(value)?Array.from(value):value]}catch(e){return ['error',e.name,e.code??null,e.syscall??null,e.message]}}
try{process.chdir(dir);
 for(const options of [undefined,null,false,true,0,42,Symbol('x'),[],{},function(){},'utf8','UTF8','','buffer','bad',{encoding:'bad'},{encoding:1},{encoding:false},{flag:'bad'},{flag:1.5},{flag:'r+'},{flag:'sr'},{flag:0},{flag:null},{signal:1},{buffer:null},{buffer:1},{buffer:{}},{buffer:[]}])out.push(capture(options));
 const fn=function(){};Object.defineProperty(fn,'encoding',{get(){throw Error('ignored function options')}});out.push(capture(fn));
 for(const encoding of [undefined,'utf8']){
  const trace=[],marker={},input=Buffer.from('plain');input.valueOf=function(){trace.push('valueOf');return this};input.toString=function(){trace.push('toString');return 'plain'};
  const options={get encoding(){trace.push('encoding');return encoding},get signal(){trace.push('signal');return undefined},get buffer(){trace.push('buffer');return undefined},get flag(){trace.push('flag');throw marker}};
  assert.throws(()=>fs.readFileSync(input,options),e=>e===marker);out.push([encoding??null,trace]);
 }
 for(const encoding of [undefined,'utf8']){const trace=[],options={encoding,get flag(){trace.push('flag');return 'bad'}};out.push([encoding??null,capture(options,'a\0b'),trace]);}
 fs.writeFileSync('plain','data');const fd=fs.openSync('plain','r');try{const marker={};out.push(fs.readFileSync(fd,{encoding:'ascii',get flag(){throw marker}}));assert.throws(()=>fs.readFileSync(fd,{encoding:'utf8',get flag(){throw marker}}),e=>e===marker);}finally{fs.closeSync(fd);}
 const isEncoding=Buffer.isEncoding,calls=[];try{Buffer.isEncoding=function(value){calls.push(value);return true};out.push(capture({encoding:'bad'},'missing'));}finally{Buffer.isEncoding=isEncoding;}out.push(calls);
}finally{process.chdir(original);fs.rmSync(dir,{recursive:true,force:true});}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn read_file_supplied_buffers_factories_aliases_and_cleanup_match_node() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert'),out=[];
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-buffers-')),file=path.join(dir,'file'),empty=path.join(dir,'empty');
fs.writeFileSync(file,'abcdef');fs.writeFileSync(empty,'');
function capture(operation){try{const value=operation();return ['value',Buffer.isBuffer(value),typeof value==='string'?value:Array.from(value)]}catch(e){return ['error',e.name,e.code??null,e.syscall??null,e.message]}}
try{
 for(const kind of ['Buffer','Uint8Array','Uint16Array','DataView']){
  const backing=new ArrayBuffer(20),view=kind==='Buffer'?Buffer.from(backing,2,12):kind==='Uint8Array'?new Uint8Array(backing,2,12):kind==='Uint16Array'?new Uint16Array(backing,2,6):new DataView(backing,2,12);
  const result=fs.readFileSync(file,{buffer:view});assert(Buffer.isBuffer(result));assert.strictEqual(result.buffer,backing);assert.strictEqual(result.byteOffset,2);assert.strictEqual(result.length,6);assert.strictEqual(result.toString(),'abcdef');out.push([kind,Array.from(new Uint8Array(backing)),result.length]);
 }
 const options={buffer(size){assert.strictEqual(this,options);out.push(['factory',size]);fs.writeFileSync(file,'UVWXYZmore');return new Uint8Array(12)}};out.push(capture(()=>fs.readFileSync(file,options)));fs.writeFileSync(file,'abcdef');
 for(const buffer of [Buffer.alloc(3),new Uint8Array(3),()=>Buffer.alloc(3),()=>null,()=>({}),()=>{throw 'factory-marker'}])out.push(capture(()=>fs.readFileSync(file,{buffer})));
 for(const size of [0,2])out.push(capture(()=>fs.readFileSync(empty,{buffer:Buffer.alloc(size)})));
 out.push(capture(()=>fs.readFileSync(file,{encoding:'utf8',buffer:Buffer.alloc(8)})));
 const originalOpen=fs.openSync,originalRead=fs.readSync,originalClose=fs.closeSync,trace=[];let opened;
 try{
  fs.openSync=function(...args){trace.push('open');opened=Reflect.apply(originalOpen,this,args);return opened};
  fs.readSync=function(...args){trace.push('read');return Reflect.apply(originalRead,this,args)};
  fs.closeSync=function(...args){trace.push('close');return Reflect.apply(originalClose,this,args)};
  out.push(capture(()=>fs.readFileSync(file,{buffer(size){trace.push('factory:'+size);return Buffer.alloc(8)}})));assert.throws(()=>fs.fstatSync(opened),e=>e.code==='EBADF');out.push(trace.splice(0));
  const marker={};assert.throws(()=>fs.readFileSync(file,{buffer(){throw marker}}),e=>e===marker);assert.throws(()=>fs.fstatSync(opened),e=>e.code==='EBADF');out.push(trace.splice(0));
 }finally{fs.openSync=originalOpen;fs.readSync=originalRead;fs.closeSync=originalClose;}
 const fd=fs.openSync(file,'r');try{const marker={};assert.throws(()=>fs.readFileSync(fd,{buffer(){throw marker}}),e=>e===marker);assert.strictEqual(fs.fstatSync(fd).size,6);out.push(capture(()=>fs.readFileSync(fd,{buffer:Buffer.alloc(8)})));}finally{fs.closeSync(fd);}
 out.push(capture(()=>fs.readFileSync(dir,{buffer:Buffer.alloc(8)})));
}finally{fs.rmSync(dir,{recursive:true,force:true});}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn read_sync_copies_raw_bytes_without_public_set_or_metadata_hooks() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert');
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-copy-')),file=path.join(directory,'file'),output=[];
fs.writeFileSync(file,'abcdef');
try {
 for (const kind of ['Uint8Array','Uint16Array','Int32Array','Float64Array','BigInt64Array','DataView']) {
  const backing=new ArrayBuffer(32),view=kind==='DataView'?new DataView(backing,8,16):new globalThis[kind](backing,8,16/globalThis[kind].BYTES_PER_ELEMENT);
  for(const name of ['buffer','byteOffset','set']) Object.defineProperty(view,name,{get(){throw new Error('public '+name)}});
  const fd=fs.openSync(file,'r');try {
   const count=fs.readSync(fd,view,2,4,0);assert.strictEqual(count,4);output.push([kind,Array.from(new Uint8Array(backing)),fs.readSync(fd,view,0,2,6)]);
  }finally{fs.closeSync(fd)}
 }
 const fd=fs.openSync(file,'r'),buffer=Buffer.alloc(8);try {
  Object.defineProperty(buffer,'set',{value(){throw new Error('public Buffer.set')}});
  output.push(['buffer',fs.readSync(fd,buffer,1,6,0),Array.from(buffer)]);
 }finally{fs.closeSync(fd)}
}finally{fs.rmSync(directory,{recursive:true,force:true})}
console.log(JSON.stringify(output));
"#,
    );
}

#[test]
fn error_capture_stack_trace_primitive_targets_match_node() {
    compare_node_stream_source(
        r#"
const out=[];for(const target of [undefined,null,1,'x',true,1n,Symbol('x')]){
 try{Error.captureStackTrace(target);out.push('accepted')}catch(error){out.push([error.name,error.message,Reflect.ownKeys(error).filter(k=>k!=='stack')])}
}console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn read_sync_overloads_validation_order_and_bigint_positions_match_node() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert'),out=[];
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-policy-')),file=path.join(directory,'file');fs.writeFileSync(file,'abcdef');
function capture(args){try{return ['value',fs.readSync(...args)]}catch(e){return ['error',e.name,e.code??null,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack').map(String),Reflect.ownKeys(Object.getPrototypeOf(e)).map(String)]}}
try {
 for(const fd of [-1,999999,{},'1',null,1.5])for(const buffer of [{},Buffer.alloc(1)])for(const length of [0,1,undefined])out.push(capture([fd,buffer,0,length,0]));
 for(const offset of [undefined,null,0,-1,1.5,'0',{},[],()=>0])out.push(capture([-1,Buffer.alloc(2),offset]));
 for(const length of [undefined,null,0,-1,1.5,'1',NaN,Infinity,2**32,2**32+1,1n,Symbol('length')])out.push(capture([-1,Buffer.alloc(2),0,length,0]));
 for(const position of [undefined,null,-1,0,-2,1.5,'0',0n,-1n,3n,2n**63n-1n,2n**63n]){
  const fd=fs.openSync(file,'r'),buffer=Buffer.alloc(2);try{out.push([capture([fd,buffer,0,2,position]),Array.from(buffer),fs.readFileSync(fd,'utf8')])}finally{fs.closeSync(fd)}
 }
 const trace=[],marker={};let buffer=Buffer.alloc(2);
 let options={get offset(){trace.push('offset');return -1},get length(){trace.push('length');throw marker},get position(){trace.push('position');return 0}};
 assert.throws(()=>fs.readSync(-1,buffer,options),e=>e===marker);out.push(trace.splice(0));
 options={get offset(){trace.push('offset');return 0},get length(){trace.push('length');return {valueOf(){trace.push('coerce length');return 0}}},get position(){trace.push('position');return null}};
 out.push(capture([{},buffer,options]),trace.splice(0));
 for(const name of ['byteLength'])Object.defineProperty(buffer,name,{get(){trace.push(name);return 2}});
 out.push(capture([-1,buffer,{offset:0,length:0}]),trace.splice(0));
 out.push(capture([-1,buffer,{offset:0,length:1}]),trace.splice(0));
}finally{fs.rmSync(directory,{recursive:true,force:true})}
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn read_file_unknown_size_supplied_buffer_errors_preserve_node_prototypes() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),out=[];
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-unknown-')),file=path.join(directory,'empty');fs.writeFileSync(file,'');
try{for(const size of [0,3,6,7]){
 const fd=fs.openSync(file,'r'),original=fs.readSync,source=Buffer.from('abcdef');let pos=0;
 fs.readSync=function(fd,buffer,offset,length){const n=Math.min(length,source.length-pos);buffer.set(source.subarray(pos,pos+n),offset);pos+=n;return n};
 try{const b=fs.readFileSync(fd,{buffer:Buffer.alloc(size)});out.push(['value',b.toString(),pos])}catch(e){out.push(['error',e.name,e.code,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack'),Object.getPrototypeOf(e).constructor.name,Reflect.ownKeys(Object.getPrototypeOf(e)),pos])}finally{fs.readSync=original;fs.closeSync(fd)}
}}finally{fs.rmSync(directory,{recursive:true,force:true})}console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn read_sync_positional_directory_errors_preserve_kernel_errno() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),out=[],fd=fs.openSync('/tmp','r');try{for(const position of [null,0,2**32,2n**40n])try{out.push(['value',fs.readSync(fd,Buffer.alloc(1),0,1,position)])}catch(e){out.push([e.name,e.code,e.syscall,e.message])}}finally{fs.closeSync(fd)}console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn application_internal_module_names_do_not_receive_node_bootstrap_transforms() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("package.json"),
        r#"{"type":"commonjs"}"#,
    )
    .unwrap();
    for (name, source) in [
        ("crypto/util.js", "const kHandle = Symbol('kHandle'); module.exports=[kHandle.description,Symbol.keyFor(kHandle)===undefined];"),
        ("per_context/primordials.js", "module.exports='application-primordials';"),
        ("bootstrap/realm.js", "module.exports=[typeof getInternalBinding,typeof getLinkedBinding];"),
    ] {
        let file=directory.path().join("internal").join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file,source).unwrap();
    }
    let source=format!("const p={};console.log(JSON.stringify(['crypto/util','per_context/primordials','bootstrap/realm'].map(name=>require(p+'/internal/'+name+'.js'))));",serde_json::to_string(&directory.path().to_string_lossy()).unwrap());
    compare_node_stream_source(&source);
}

#[test]
fn crypto_hash_handle_symbols_are_private_canonical_and_not_public_registry_keys() {
    compare_node_source_with_args(
        r#"
const assert=require('node:assert'),crypto=require('node:crypto'),utility=require('internal/crypto/util'),key=utility.kHandle;
assert.strictEqual(key.description,'kHandle');assert.strictEqual(Symbol.keyFor(key),undefined);
const first=crypto.createHash('sha256');assert(Object.getOwnPropertySymbols(first).includes(key));
const replacement=Symbol('replacement');utility.kHandle=replacement;
try {
 const second=crypto.createHash('sha256');assert(Object.getOwnPropertySymbols(second).includes(key));assert(!Object.getOwnPropertySymbols(second).includes(replacement));
 assert.strictEqual(first.update('abc').digest('hex'),second.update('abc').digest('hex'));
 console.log(key.description,Symbol.keyFor(key)===undefined,first instanceof crypto.Hash);
}finally{utility.kHandle=key;}
"#,
        &["--expose-internals"],
    );
}

#[test]
fn write_sync_raw_views_positions_encoding_fallbacks_and_native_errors_match_node() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),out=[];
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-write-policy-')),file=path.join(dir,'file');
function error(e){return ['error',e.name,e.code??null,e.syscall??null,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack').map(String),Reflect.ownKeys(Object.getPrototypeOf(e)).map(String)]}
try{
 for(const kind of ['Uint8Array','Uint16Array','Int32Array','DataView']){
  fs.writeFileSync(file,'--------');const fd=fs.openSync(file,'r+'),backing=new ArrayBuffer(12),view=kind==='DataView'?new DataView(backing,4,8):new globalThis[kind](backing,4,8/globalThis[kind].BYTES_PER_ELEMENT);new Uint8Array(backing).set([65,66,67,68,69,70,71,72],4);
  for(const name of ['buffer','byteOffset','subarray'])Object.defineProperty(view,name,{get(){throw new Error('public '+name)}});
  try{out.push([kind,fs.writeSync(fd,view,2,4,1),fs.readFileSync(file,'utf8')])}catch(e){out.push([kind,error(e)])}finally{fs.closeSync(fd)}
 }
 for(const position of [undefined,null,-1,-2,0,1,1.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1,'2',2n,{}]){
  fs.writeFileSync(file,'abcdef');const fd=fs.openSync(file,'r+');try{fs.readSync(fd,Buffer.alloc(3),0,3,null);out.push(['position',typeof position,(()=>{try{return fs.writeSync(fd,'X',position,'utf8')}catch(e){return error(e)}})(),fs.readFileSync(file,'utf8'),fs.readFileSync(fd,'utf8')])}finally{fs.closeSync(fd)}
 }
 for(const data of [{},null,1,new Uint8Array(0),'x'])for(const fd of [-1,{},999999])try{out.push(['fd',fs.writeSync(fd,data,0,0,0)])}catch(e){out.push(error(e))}
 for(const encoding of [undefined,null,'','bad','buffer','hex','HEX',1,{},Symbol('encoding')]){
  fs.writeFileSync(file,'');const fd=fs.openSync(file,'r+');try{out.push(['encoding',(()=>{try{return fs.writeSync(fd,'éA',null,encoding)}catch(e){return error(e)}})(),Array.from(fs.readFileSync(file))])}finally{fs.closeSync(fd)}
 }
}finally{fs.rmSync(dir,{recursive:true,force:true})}console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn write_sync_reports_partial_nonblocking_pipe_progress_and_kernel_errors() {
    let directory = tempfile::tempdir().unwrap();
    let fifo = directory.path().join("pipe");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let source = format!(
        r#"
const fs=require('node:fs'),assert=require('node:assert'),fd=fs.openSync({},fs.constants.O_RDWR|fs.constants.O_NONBLOCK);
try {{
 const buffer=Buffer.alloc(1024*1024,90),count=fs.writeSync(fd,buffer);
 assert(count>0&&count<buffer.length);
 let failure;try{{fs.writeSync(fd,buffer)}}catch(error){{failure=error;}}
 assert.strictEqual(failure.code,'EAGAIN');assert.strictEqual(failure.syscall,'write');assert.strictEqual(failure.path,undefined);
 const result=Buffer.alloc(count);assert.strictEqual(fs.readSync(fd,result,0,count,null),count);assert(result.every(value=>value===90));
 console.log('partial',typeof count,failure.code,failure.syscall,result.length===count);
}}finally{{fs.closeSync(fd)}}
"#,
        serde_json::to_string(&fifo.to_string_lossy()).unwrap()
    );
    compare_node_stream_source(&source);
}

#[cfg(unix)]
#[test]
fn filesystem_errors_use_platform_errno_names_for_loops_long_names_and_empty_pipes() {
    let directory = tempfile::tempdir().unwrap();
    let loop_path = directory.path().join("loop");
    std::os::unix::fs::symlink("loop", &loop_path).unwrap();
    let fifo = directory.path().join("pipe");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let source = format!(
        r#"
const fs=require('node:fs'),path=require('node:path'),out=[];
const directory={},loopPath=path.join(directory,'loop'),longPath=path.join(directory,'x'.repeat(300));
for(const file of [loopPath,longPath])for(const operation of [()=>fs.statSync(file),()=>fs.openSync(file,'r'),()=>fs.readFileSync(file,'utf8')]){{
 try{{operation();out.push('unexpected')}}catch(error){{out.push([error.name,error.code,error.errno,error.syscall,error.message.replaceAll(directory,'<directory>')]);}}
}}
const fd=fs.openSync(path.join(directory,'pipe'),fs.constants.O_RDWR|fs.constants.O_NONBLOCK);
try {{try{{fs.readSync(fd,Buffer.alloc(1),0,1,null)}}catch(error){{out.push([error.name,error.code,error.errno,error.syscall,error.message]);}}}}finally{{fs.closeSync(fd)}}
console.log(JSON.stringify(out));
"#,
        serde_json::to_string(&directory.path().to_string_lossy()).unwrap()
    );
    compare_node_stream_source(&source);
}

#[test]
fn descriptor_read_write_forwarding_does_not_invoke_public_argument_iterators() {
    compare_node_stream_source(
        r#"
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');const directory=fs.mkdtempSync(path.join(os.tmpdir(),'quench-fs-iterator-')),file=path.join(directory,'file');fs.writeFileSync(file,'---');
const fd=fs.openSync(file,'r+'),source=Buffer.from('Q'),target=Buffer.alloc(1),prototype=Object.getPrototypeOf([][Symbol.iterator]()),descriptor=Object.getOwnPropertyDescriptor(prototype,'next'),marker={},out=[],poison=function(){throw marker};
try {
 Object.defineProperty(prototype,'next',{...descriptor,value:poison});
 try{out[0]=fs.writeSync(fd,source,0,1,0)}catch(error){out[0]=error===marker?'iterator-marker':error.message}
 out[3]=prototype.next===poison;
 try{out[1]=fs.readSync(fd,target,0,1,0)}catch(error){out[1]=error===marker?'iterator-marker':error.message}
 out[4]=prototype.next===poison;
}finally{Object.defineProperty(prototype,'next',descriptor);fs.closeSync(fd);fs.rmSync(directory,{recursive:true,force:true})}
out[2]=target[0];console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(target_os = "linux")]
const PROCESS_MEMORY_SOURCE: &str = r#"
const fs=require('node:fs'),os=require('node:os'),assert=require('node:assert'),processModule=require('node:process'),out=[];
const physical=os.totalmem();
for(const name of ['availableMemory','constrainedMemory']){
 const fn=processModule[name],descriptor=Object.getOwnPropertyDescriptor(processModule,name);
 out.push([name,fn.name,fn.length,Object.getOwnPropertyNames(fn),descriptor.writable,descriptor.enumerable,descriptor.configurable]);
 const value=Reflect.apply(fn,{unrelated:true},[{valueOf(){throw Error('argument coercion')}}]);
 assert(Number.isFinite(value)&&value>=0);
 if(name==='availableMemory')assert(value<=physical);
 else out.push(['constraint',value]);
 assert.throws(()=>Reflect.construct(fn,[]),TypeError);
}
const original=fs.readFileSync;fs.readFileSync=()=>{throw Error('public fs hook')};
try{assert.strictEqual(typeof processModule.availableMemory(),'number');assert.strictEqual(typeof processModule.constrainedMemory(),'number');out.push('native facts')}finally{fs.readFileSync=original}
console.log(JSON.stringify(out));
"#;
#[cfg(target_os = "linux")]
#[test]
fn process_memory_reports_kernel_constraints_and_native_function_metadata() {
    compare_node_stream_source(PROCESS_MEMORY_SOURCE);
}
#[cfg(target_os = "linux")]
#[test]
fn process_memory_usage_reports_heap_and_external_backing_stores_like_node() {
    compare_node_stream_source(
        r#"
const memoryUsage=process.memoryUsage,descriptor=Object.getOwnPropertyDescriptor(process,'memoryUsage'),out=[];
out.push([memoryUsage.name,memoryUsage.length,Object.keys(memoryUsage),descriptor.writable,descriptor.enumerable,descriptor.configurable,memoryUsage.rss.name]);
const before=memoryUsage();
out.push(Object.keys(before),Object.values(before).every(Number.isSafeInteger));
out.push([before.rss>0,memoryUsage.rss()>0,before.heapTotal>=before.heapUsed,before.heapUsed>0,before.external>=before.arrayBuffers]);
const size=8*1024*1024,buffer=new ArrayBuffer(size),after=memoryUsage();
out.push([after.external-before.external>=size,after.arrayBuffers>=size,buffer.byteLength]);
console.log(JSON.stringify(out));
"#,
    );
}
#[cfg(target_os = "linux")]
#[test]
fn process_memory_constraints_respect_inherited_data_limits() {
    use std::os::unix::process::CommandExt;
    const DATA_LIMIT_BYTES: u64 = 12 * 1024 * 1024 * 1024;
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", PROCESS_MEMORY_SOURCE]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        // The lower soft limit applies only to this test child, leaving the runner unchanged.
        unsafe {
            command.pre_exec(|| {
                let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
                if libc::getrlimit(libc::RLIMIT_DATA, limit.as_mut_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let mut limit = limit.assume_init();
                limit.rlim_cur = limit.rlim_cur.min(DATA_LIMIT_BYTES as libc::rlim_t);
                if libc::setrlimit(libc::RLIMIT_DATA, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(
            observation.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[cfg(unix)]
#[test]
fn os_memory_functions_report_native_values_names_and_primitive_conversion() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),OriginalSymbol=globalThis.Symbol;globalThis.Symbol=function(){throw Error('public Symbol')};let os;
try{os=require('node:os')}finally{globalThis.Symbol=OriginalSymbol}
const out=[],physical=os.totalmem();
for(const name of ['freemem','totalmem']){
 const f=os[name],d=Object.getOwnPropertyDescriptor(f,Symbol.toPrimitive),hook=f[Symbol.toPrimitive];
 const value=f();assert(Number.isFinite(value)&&value>=0&&value<=physical);
 out.push([name,f.name,f.length,Object.getOwnPropertyNames(f),Reflect.ownKeys(f).map(String),d.writable,d.enumerable,d.configurable,hook.name,hook.length,hook===f]);
 assert.strictEqual(Function.prototype.toString.call(f),'function () { [native code] }');
 assert.strictEqual(f.valueOf(),f);assert.throws(()=>Reflect.construct(f,[]),TypeError);
 assert.strictEqual(typeof +f,'number');assert(/^\d+$/.test(String(f)));
 const original=os[name];os[name]=()=>-1;
 try{const number=original[Symbol.toPrimitive].call({unrelated:true},Symbol('hint'));assert(number>=0&&number<=physical);out.push('captured native value')}finally{os[name]=original}
}
console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn os_tmpdir_uses_live_nonempty_environment_precedence_and_removes_one_slash() {
    compare_node_stream_source(
        r#"
const os=require('node:os'),out=[];
for(const values of [[undefined,undefined,undefined],['','/tmp-value','/temp-value'],['','','/temp-value'],['','',''],['a///','b','c'],['/','b','c'],['//','b','c'],['a\\','b','c'],['relative/','b','c'],['😀///','b','c']]){
 for(const [i,key] of ['TMPDIR','TMP','TEMP'].entries()){if(values[i]===undefined)delete process.env[key];else process.env[key]=values[i]}
 out.push(os.tmpdir());
}
const environment=process.env;process.env={TMPDIR:'/replacement'};
try{out.push(os.tmpdir())}finally{process.env=environment}
process.env.TMPDIR='';process.env.TMP='';process.env.TEMP='';process.env.TEMPDIR='/ignored';out.push(os.tmpdir());
console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn os_canonical_exports_reflection_constants_and_stable_host_values_match_node() {
    compare_node_stream_source(
        r#"
const os=require('node:os'),out=[];
for(const key of Reflect.ownKeys(os)){
 const d=Object.getOwnPropertyDescriptor(os,key),v=os[key];
 out.push([String(key),d.writable,d.enumerable,d.configurable,typeof v]);
 if(typeof v==='function'){
  let constructible=true;try{Reflect.construct(Object,[],v)}catch{constructible=false}
  out.push([v.name,v.length,constructible,Reflect.ownKeys(v).map(String),Object.getOwnPropertyNames(v).map(k=>{const d=Object.getOwnPropertyDescriptor(v,k);return[k,d.writable,d.enumerable,d.configurable]}),Function.prototype.toString.call(v)]);
  if(v[Symbol.toPrimitive])out.push([v[Symbol.toPrimitive].name,v[Symbol.toPrimitive].length,Function.prototype.toString.call(v[Symbol.toPrimitive])]);
 }
}
out.push(os.constants, Object.getPrototypeOf(os.constants)===null);
for(const k of ['dlopen','errno','signals','priority'])out.push([k,Object.getPrototypeOf(os.constants[k])===null,Object.isFrozen(os.constants[k])]);
out.push(os.networkInterfaces(),os.userInfo(),os.homedir(),os.hostname(),os.machine(),os.version(),os.release(),os.type(),os.arch(),os.platform(),os.endianness(),os.availableParallelism());
console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn os_user_info_reads_account_records_and_native_encodings_without_public_hooks() {
    compare_node_stream_source(
        r#"
const os=require('node:os'),out=[];
const original=Buffer.from;Buffer.from=()=>{throw Error('public Buffer.from')};
try{
 for(const encoding of [undefined,'utf8','UTF-8','hex','base64','base64url','ascii','latin1','binary','utf16le','ucs2','buffer','BUFFER','not-encoding',null,0,false,{},new String('hex')]){
  const events=[],value=os.userInfo({get encoding(){events.push('encoding');return encoding}});
  out.push([events,Object.getPrototypeOf(value)===null,Object.keys(value),value]);
 }
 for(const options of [undefined,null,1,'hex',()=>{}])out.push(os.userInfo(options));
 for(const thrown of [17,'error',Symbol.for('marker'),new Error('getter')]){
  try{os.userInfo({get encoding(){throw thrown}})}catch(e){out.push(e===thrown)}
 }
 const environment=process.env;
 for(const home of ['', '/different-home','😀///']){process.env.HOME=home;out.push([os.homedir(),os.userInfo().homedir])}
 delete process.env.HOME;out.push([os.homedir(),os.userInfo().homedir]);
}finally{Buffer.from=original}
console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(unix)]
#[test]
fn os_priority_validation_native_system_errors_and_process_effects_match_node() {
    compare_node_stream_source(
        r#"
const os=require('node:os'),out=[];
const record=f=>{try{out.push(['ok',f()])}catch(e){out.push([e.name,e.message,e.code,e.info,Reflect.ownKeys(e).map(String),e.constructor.name,Object.getPrototypeOf(e).constructor.name])}};
for(const pid of [null,true,false,'foo',{},[],/x/,NaN,Infinity,-Infinity,3.14,2**32,-1,2147483647]){
 record(()=>os.getPriority(pid));record(()=>os.setPriority(pid,0));
}
for(const p of [null,true,'foo',{},[],NaN,Infinity,-Infinity,3.14,2**32,-21,20,undefined])record(()=>os.setPriority(0,p));
const before=os.getPriority();os.setPriority(Math.min(19,before+1));out.push(os.getPriority()===Math.min(19,before+1));
os.setPriority(process.pid,Math.min(19,before+2));out.push(os.getPriority()===Math.min(19,before+2));
const child=require('node:child_process').spawnSync(process.execPath,['-e','console.log(require("node:os").getPriority())'],{encoding:'utf8'});out.push([child.status,child.stdout.trim()===String(Math.min(19,before+2)),child.stderr]);
console.log(JSON.stringify(out));
"#,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn os_load_and_uptime_use_kernel_data_and_return_fresh_results() {
    compare_node_stream_source(
        r#"
const assert=require('node:assert'),os=require('node:os'),fs=require('node:fs');
const before=Number(fs.readFileSync('/proc/uptime','utf8').split(' ')[0]),actual=os.uptime(),after=Number(fs.readFileSync('/proc/uptime','utf8').split(' ')[0]);assert(actual>=before&&actual<=after);
const first=os.loadavg();assert(Array.isArray(first)&&first.length===3&&first.every(x=>typeof x==='number'&&Number.isFinite(x)&&x>=0));
first.fill(-1);const current=os.loadavg();assert(current.every(x=>x>=0));assert.notStrictEqual(first,current);
const kernel=fs.readFileSync('/proc/loadavg','utf8').trim().split(/\s+/).slice(0,3).map(Number);
assert(current.every((x,i)=>Math.abs(x-kernel[i])<1));
console.log('live os facts');
"#,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn os_priority_changes_are_visible_to_the_kernel_on_the_child_process_pid() {
    struct RunningChild(std::process::Child);
    impl Drop for RunningChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("ready.json");
    let child = Command::new(env!("CARGO_BIN_EXE_run-parallel"))
        .env(quench_node::shared_run::CHILD_RUNNER_ENV, "1")
        .args(["-e", "const os=require('node:os');const priority=Math.min(19,os.getPriority()+1);os.setPriority(priority);require('node:fs').writeFileSync(process.argv[1],JSON.stringify(priority));setInterval(()=>{},1000)"])
        .arg(&marker)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut child = RunningChild(child);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "priority child exited before readiness"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "priority child did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let expected: i32 = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    unsafe {
        *libc::__errno_location() = 0;
    }
    let actual = unsafe { libc::getpriority(libc::PRIO_PROCESS, child.0.id()) };
    assert_eq!(unsafe { *libc::__errno_location() }, 0);
    assert_eq!(actual, expected);
}

#[test]
fn native_vfs_descriptors_share_live_positions_registry_identity_and_close_ownership() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-vfs-descriptors.js"),
        &["--no-warnings", "--experimental-vfs", "--expose-internals"],
    );
}

#[test]
fn scalar_assertion_guards_preserve_aliases_metadata_success_effects_and_failure_policy() {
    compare_node_stream_source(include_str!("../node-tests/test-assert-scalar-guards.js"));
}

#[test]
fn events_async_iterator_preserves_queues_cleanup_abort_and_watermarks() {
    let observations = compare_node_stream_source(r#"
const assert=require('assert'),{EventEmitter,on}=require('events');
(async()=>{
 const e=new EventEmitter(),flow=[];
 e.pause=()=>flow.push('pause');e.resume=()=>flow.push('resume');
 const iterator=on(e,'data',{close:['end'],highWaterMark:1,lowWaterMark:1});
 assert.strictEqual(iterator[Symbol.asyncIterator](),iterator);
 e.emit('data',1,'a');e.emit('data',2,'b');
 assert.deepStrictEqual(flow,['pause']);
 assert.deepStrictEqual(await iterator.next(),{value:[1,'a'],done:false});
 assert.deepStrictEqual(await iterator.next(),{value:[2,'b'],done:false});
 assert.deepStrictEqual(flow,['pause','resume']);
 const pending=iterator.next();e.emit('end');assert.strictEqual((await pending).done,true);
 assert.strictEqual(e.listenerCount('data')+e.listenerCount('error')+e.listenerCount('end'),0);
 const target=new EventTarget(),targetIterator=on(target,'value');
 const targetPending=targetIterator.next(),event=new Event('value');target.dispatchEvent(event);
 assert.strictEqual((await targetPending).value[0],event);await targetIterator.return();
 const controller=new AbortController();controller.signal.addEventListener('abort',event=>event.stopImmediatePropagation());
 const aborted=on(e,'data',{signal:controller.signal});
 const wait=aborted.next();controller.abort('reason');
 await assert.rejects(wait,error=>error.code==='ABORT_ERR'&&error.cause==='reason');
 assert.strictEqual((await aborted.next()).done,true);
 const failed=on(e,'data'),first=failed.next(),second=failed.next(),error=new Error('failure');
 e.emit('error',error);await assert.rejects(first,err=>err===error);
 assert.strictEqual((await second).done,true);
 const returned=on(e,'data');assert.strictEqual((await returned.return()).done,true);
 const thrown=on(e,'data');assert.strictEqual(thrown.throw(error),undefined);
 await assert.rejects(thrown.next(),err=>err===error);
 for(const options of [null,1,{highWaterMark:0},{lowWatermark:0}])assert.throws(()=>on(e,'data',options));
 controller.abort();assert.throws(()=>on(e,'data',{signal:controller.signal}),error=>error.code==='ABORT_ERR');
 assert.strictEqual(e.listenerCount('data')+e.listenerCount('error'),0);
 console.log(JSON.stringify({complete:true}));
})().catch(error=>{console.error(error);process.exitCode=1});
"#);
    assert_eq!(String::from_utf8_lossy(&observations[1].stdout).trim(), r#"{"complete":true}"#);
}

#[test]
fn events_async_iterator_descriptor_initialization_captures_original_abort_helper() {
    let observations=compare_node_stream_source(r#"
const assert=require('assert'),events=require('events');
const marker={};events.addAbortListener=()=>{throw marker};
const originalPromise=globalThis.Promise,originalApply=Reflect.apply;
globalThis.Promise=function(){throw marker};Reflect.apply=()=>{throw marker};
let descriptor;
try{descriptor=Object.getOwnPropertyDescriptor(events,'on')}finally{globalThis.Promise=originalPromise;Reflect.apply=originalApply}
assert.strictEqual(typeof descriptor.value,'function');assert.strictEqual(descriptor.get,undefined);
assert.strictEqual(descriptor.writable,true);assert.strictEqual(descriptor.enumerable,true);assert.strictEqual(descriptor.configurable,true);
assert.strictEqual(descriptor.value.name,'on');assert.strictEqual(descriptor.value.length,2);
assert.strictEqual(events.on,descriptor.value);Object.freeze(events);assert.strictEqual(events.on,descriptor.value);
(async()=>{
 const controller=new AbortController();controller.signal.addEventListener('abort',event=>event.stopImmediatePropagation());
 const iterator=events.on(new events.EventEmitter(),'data',{signal:controller.signal});
 const pending=iterator.next();controller.abort('reason');
 await assert.rejects(pending,error=>error.code==='ABORT_ERR'&&error.cause==='reason');
 console.log(JSON.stringify({complete:true}));
})().catch(error=>{console.error(error);process.exitCode=1});
"#);
    assert_eq!(String::from_utf8_lossy(&observations[1].stdout).trim(),r#"{"complete":true}"#);
}

#[test]
fn events_async_iterator_initialization_preserves_freeze_before_observation() {
    let observations=compare_node_stream_source(r#"
const assert=require('assert'),events=require('events');Object.freeze(events);
const descriptor=Object.getOwnPropertyDescriptor(events,'on');assert.strictEqual(typeof descriptor.value,'function');
assert.strictEqual(descriptor.writable,false);assert.strictEqual(descriptor.configurable,false);assert.strictEqual(descriptor.enumerable,true);
assert.strictEqual(events.on,descriptor.value);
(async()=>{const emitter=new events.EventEmitter(),iterator=events.on(emitter,'data');emitter.emit('data',42);assert.deepStrictEqual((await iterator.next()).value,[42]);await iterator.return();console.log(JSON.stringify({complete:true}));})().catch(error=>{console.error(error);process.exitCode=1});
"#);
    assert_eq!(String::from_utf8_lossy(&observations[1].stdout).trim(),r#"{"complete":true}"#);
}

#[test]
fn child_stdin_destruction_closes_pipe_and_precedes_exit_observers() {
    let observations = compare_node_stream_source(r#"
const assert=require('assert'),{spawn}=require('child_process');
const child=spawn(process.execPath,['-e',`process.stdin.resume();process.stdin.on('end',()=>process.exit(0));`]);
const original=child.stdin.destroy;let calls=0,exited=false;
child.stdin.destroy=function(...args){calls++;return Reflect.apply(original,this,args)};
child.on('error',error=>{throw error});
child.on('spawn',()=>child.stdin.destroy());
child.on('exit',(code,signal)=>{assert.strictEqual(code,0);assert.strictEqual(signal,null);assert(child.stdin.destroyed);assert(calls>=2);assert.strictEqual(child._handle,null);exited=true});
child.on('close',()=>{assert(exited);console.log(JSON.stringify({complete:true}))});
"#);
    assert_eq!(String::from_utf8_lossy(&observations[1].stdout).trim(), r#"{"complete":true}"#);
}

#[test]
fn ipc_counted_references_preserve_initial_state_explicit_overrides_and_exit() {
    for (methods, kept) in [
        ("['unrefCounted']", true),
        ("['refCounted','unrefCounted']", false),
        ("['refCounted','refCounted','unrefCounted']", true),
        ("['unref','refCounted','refCounted']", false),
        ("['ref','unrefCounted']", true),
        ("['unrefCounted','refCounted','refCounted','unrefCounted']", false),
    ] {
        let source = format!(r#"
const assert=require('assert'),{{spawn}}=require('child_process');
const child=spawn(process.execPath,['-e','process.on("message",()=>process.exit(0))'],{{stdio:['ignore','ignore','ignore','ipc']}});
child.unref();
for(const name of ['refCounted','unrefCounted']){{
 const method=child.channel[name],descriptor=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(child.channel),name);
 assert.strictEqual(method.name,name);assert.strictEqual(method.length,0);
 assert.strictEqual(descriptor.enumerable,false);assert.strictEqual(descriptor.writable,true);assert.strictEqual(descriptor.configurable,true);
 assert.throws(()=>method.call({{}}),TypeError);assert.throws(()=>Reflect.construct(method,[]),TypeError);
}}
for(const method of {methods})assert.strictEqual(child.channel[method](),undefined);
console.log('setup');setTimeout(()=>{{console.log('kept');child.send('done')}},50).unref();
"#);
        let observations = compare_node_stream_source(&source);
        assert_eq!(String::from_utf8_lossy(&observations[1].stdout), if kept {"setup\nkept\n"} else {"setup\n"});
    }
}

#[test]
fn ipc_process_counted_references_compose_with_implicit_listener_counts() {
    let observations = compare_node_stream_source(r#"
const assert=require('assert'),{spawn}=require('child_process');
(async()=>{
 const out=[];
 for(const keep of [false,true]){
  const source=`process.on('message',()=>{});process.on('disconnect',()=>{});
  process.channel.unrefCounted();process.channel.unrefCounted();
  if(${keep})process.channel.refCounted();
  console.log('setup');setTimeout(()=>{console.log('kept');process.exit(0)},50).unref();`;
  const child=spawn(process.execPath,['-e',source],{stdio:['ignore','pipe','pipe','ipc']});
  let stdout='',stderr='';child.stdout.on('data',chunk=>stdout+=chunk);child.stderr.on('data',chunk=>stderr+=chunk);
  const code=await new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',resolve)});
  assert.strictEqual(code,0);assert.strictEqual(stderr,'');assert.strictEqual(stdout,keep?'setup\nkept\n':'setup\n');out.push(stdout);
 }
 console.log(JSON.stringify({complete:true,out}));
})().catch(error=>{console.error(error);process.exitCode=1});
"#);
    let result: serde_json::Value=serde_json::from_slice(&observations[1].stdout).unwrap();
    assert_eq!(result["complete"],true);
}


#[test]
fn file_url_module_imports_share_plain_aliases_and_preserve_query_identity() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("sibling.mjs"), "globalThis.siblingLoads=(globalThis.siblingLoads||0)+1;export const sibling={};").unwrap();
    let dependency=directory.path().join("module # ü.mjs");
    std::fs::write(&dependency,"import {sibling} from './sibling.mjs';export {sibling};export const id=globalThis.moduleLoads=(globalThis.moduleLoads||0)+1;export let live=1;export function increment(){live++}export const url=import.meta.url;").unwrap();
    let url=url::Url::from_file_path(&dependency).unwrap().to_string();
    let entry=directory.path().join("entry.mjs");
    std::fs::write(&entry,format!(r#"
import * as first from {literal};
import assert from 'node:assert/strict';
const base={literal};
const same=await import(base);
assert.strictEqual(first,same);
const relative=await import('./module%20%23%20%C3%BC.mjs');
assert.strictEqual(first,relative);
assert.strictEqual(first,await import(base.replace('file:','FILE:')));
same.increment();assert.strictEqual(first.live,2);
const suffixes=['?x=one','?x=two','?x=/../one','?x=/one','#part','#other','?','#'];
const out=[];
for(const suffix of suffixes){{
 const variant=await import(base+suffix);
 if(suffix==='?'||suffix==='#'){{assert.strictEqual(variant,first);continue;}}
 assert.notStrictEqual(variant,first);
 assert.strictEqual(variant,await import(base+suffix));
 assert.strictEqual(variant.sibling,first.sibling);
 assert.strictEqual(variant.url,base+suffix);
 out.push([suffix,variant.id,variant.live]);
}}
for(const [input,canonical] of [['?x=one#','?x=one'],['?#part','#part']]){{
 assert.strictEqual(await import(base+input),await import(base+canonical));
}}
assert.strictEqual(globalThis.siblingLoads,1);
assert.strictEqual(globalThis.moduleLoads,7);
console.log(JSON.stringify({{out,firstId:first.id,firstLive:first.live}}));
"#,literal=serde_json::to_string(&url).unwrap())).unwrap();
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.arg(&entry);
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1");}
        quench_node_test::case_process::observe_command(command,Duration::from_secs(20)).unwrap()
    });
    for result in &observations {assert!(!result.timed_out);assert_eq!(result.exit_code,Some(0),"{}",String::from_utf8_lossy(&result.stderr));assert!(result.stderr.is_empty());}
    assert_eq!(observations[0].stdout,observations[1].stdout);
}

#[test]
fn text_decoder_unicode_labels_bom_fatal_errors_and_recovery_match_node() {
    compare_node_stream_source(r#"
const out=[];
for(const label of ['utf8','utf-16','utf-16be','ascii'])for(const fatal of [false,true])for(const ignoreBOM of [false,true]){
 const decoder=new TextDecoder(label,{fatal,ignoreBOM});
 for(const bytes of [[],[65],[239,187,191,65],[255,254,65,0],[254,255,0,65],[0,216],[0,216,65],[195,40]]){
  try{out.push([label,fatal,ignoreBOM,decoder.encoding,decoder.decode(new Uint8Array(bytes))])}
  catch(e){out.push([label,fatal,ignoreBOM,e.name,e.code,e.message])}
 }
}
for(const label of ['bogus','replacement','iso-2022-cn','\u00a0utf-8\u00a0'])try{new TextDecoder(label)}catch(e){out.push([label,e.name,e.code,e.message])}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn synchronous_esm_require_preserves_namespaces_live_bindings_errors_and_later_imports() {
    let directory=tempfile::tempdir().unwrap();
    for (name,source) in [
        ("plain.mjs","export let n=1;export function inc(){n++}"),
        ("default.mjs","export default function f(){return 42};export let n=1;export function inc(){n++}"),
        ("marker.mjs","export default 42;export const __esModule=false;"),
        ("special.mjs","const value={answer:42};export {value as 'module.exports'};export default 7;"),
        ("async.mjs","globalThis.asyncStarted=true;await 0;export const done=true;"),
        ("parent.mjs","import './async.mjs';globalThis.parentExecuted=true;export const done=true;"),
        ("cycle.mjs","import './cycle.cjs';export const n=1;"),
        ("cycle.cjs","module.exports=require('./cycle.mjs');"),
    ] {std::fs::write(directory.path().join(name),source).unwrap();}
    let entry=directory.path().join("entry.cjs");
    std::fs::write(&entry,r#"
(async()=>{
const out=[];
for(const name of ['plain','default','marker','special']){
 const value=require('./'+name+'.mjs');const ns=await import('./'+name+'.mjs');
 out.push([name,value===require('./'+name+'.mjs'),value===ns,Object.getPrototypeOf(value)===null,Object.isExtensible(value),Reflect.ownKeys(value).map(k=>String(k))]);
 if(value.inc){value.inc();out.push([name,value.n,ns.n]);}
 if(value.default)out.push(['default',typeof value.default,value.__esModule]);
}
for(const name of ['parent','async','cycle']){
 let previous;
 for(let count=0;count<2;count++)try{require('./'+name+'.mjs');throw Error('unexpected success')}catch(e){
  out.push([name,e.name,e.code,e.message,previous===e,e.requireStack,Object.getOwnPropertyDescriptor(e,'requireStack')]);previous=e;
 }
}
out.push(['not started',globalThis.asyncStarted===undefined,globalThis.parentExecuted===undefined]);
const asyncNS=await import('./parent.mjs');out.push(['async imported',asyncNS.done,globalThis.asyncStarted,globalThis.parentExecuted]);
console.log(JSON.stringify({complete:true,out}));
})().catch(e=>{console.error(e.stack);process.exitCode=1});
"#).unwrap();
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.arg(&entry);
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1");}
        quench_node_test::case_process::observe_command(command,Duration::from_secs(20)).unwrap()
    });
    for result in &observations {
        assert!(!result.timed_out);assert_eq!(result.exit_code,Some(0),"{}",String::from_utf8_lossy(&result.stderr));assert!(result.stderr.is_empty());
        let value:serde_json::Value=serde_json::from_slice(&result.stdout).unwrap();assert_eq!(value["complete"],true);
    }
    assert_eq!(observations[0].stdout,observations[1].stdout);
}

#[test]
fn commonjs_cache_mutation_replacement_and_esm_reload_match_node() {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("value.cjs", "globalThis.loads=(globalThis.loads||0)+1;module.exports={n:loads,cache:require.cache};"),
        ("data.json", "{\"n\":1}"),
        ("failure.cjs", "throw globalThis.failure;"),
        ("default.mjs", "export default 42;"),
        ("namespace.mjs", "export const n=42;"),
    ] { std::fs::write(directory.path().join(name), source).unwrap(); }
    let entry = directory.path().join("entry.cjs");
    std::fs::write(&entry, r#"
const Module=require('node:module');const original=Module._cache;const out=[];
out.push(['identity',require.cache===original,Object.getPrototypeOf(original)===null,original[__filename]===module]);
out.push(['private',Object.keys(original).every(key=>!key.includes('/lib/internal/')&&!key.includes('\0'))]);
const filename=require.resolve('./value.cjs');const a=require('./value.cjs');
out.push(['record',original[filename].loaded,original[filename].exports===a,original[filename].parent===module]);
delete require.cache[filename];const b=require('./value.cjs');out.push(['reload',a!==b,a.n,b.n]);
const json=require.resolve('./data.json');original[json]={loaded:true,exports:{mock:42}};
out.push(['substitution',require('./data.json')]);delete original[json];out.push(['json',require('./data.json')]);
const actualFs=require('node:fs');original.fs={loaded:true,exports:{mock:'fs'}};
out.push(['builtin',require('fs'),require('node:fs')===actualFs]);delete original.fs;
const descriptor=Object.getOwnPropertyDescriptor(Module,'_cache');
out.push(['descriptor',descriptor.writable,descriptor.enumerable,descriptor.configurable]);
Module._cache=Object.create(null);const fresh=Module._cache;const c=require('./value.cjs');
out.push(['replace',require.cache===original,c.cache===fresh,c.n,fresh[filename].exports===c]);
require.cache={unused:true};out.push(['require property',require('./value.cjs')===c,Module._cache===fresh]);
require.cache=original;Module._cache=original;
globalThis.failure={tag:'failure'};const bad=require.resolve('./failure.cjs');
for(let count=0;count<2;count++)try{require('./failure.cjs')}catch(e){out.push(['failure',e===failure,Object.hasOwn(original,bad),module.children.some(child=>child.filename===bad)]);}
const events=[];const proxied=new Proxy(original,{
 get(target,key){if(key===filename)events.push('get');return Reflect.get(target,key)},
 set(target,key,value){if(key===filename)events.push('set');return Reflect.set(target,key,value)},
 deleteProperty(target,key){if(key===bad)events.push('delete');return Reflect.deleteProperty(target,key)}
});
Module._cache=proxied;delete original[filename];const d=require('./value.cjs');
try{require('./failure.cjs')}catch(e){out.push(['proxy failure',e===failure]);}
out.push(['proxy',d.cache===proxied,events]);Module._cache=original;
for(const name of ['default','namespace']){
 const path=require.resolve('./'+name+'.mjs');const first=require(path);delete original[path];const second=require(path);
 out.push(['esm',name,first===second,Object.getPrototypeOf(second)===null]);
}
console.log(JSON.stringify({complete:true,out}));
"#).unwrap();
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable); command.arg(&entry);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(20)).unwrap()
    });
    for result in &observations {
        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert!(result.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["complete"], true);
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn ambiguous_commonjs_sources_detect_esm_syntax_await_and_wrapper_bindings() {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("esm.js", "export const n=42;"),
        ("meta.js", "globalThis.detectedMeta=import.meta.url;"),
        ("lexical.js", "const require=42;globalThis.detectedLexical=require;"),
        ("destructure.js", "const {x:exports}={x:42};globalThis.detectedDestructure=exports;"),
        ("script.js", "var require;module.exports=42;return;"),
        ("string.js", "module.exports='export { imaginary }';"),
        ("await-name.js", "var await=8;module.exports=await/2/2;"),
        ("dynamic.js", "module.exports=()=>import('./esm.js');"),
        ("async.js", "globalThis.detectedStarted=true;await 0;globalThis.detectedFinished=true;"),
        ("hashbang.js", "#!/usr/bin/env node\nexport const n=42;"),
        ("bom.js", "\u{feff}export const n=42;"),
        ("common/package.json", "{\"type\":\"commonjs\"}"),
        ("common/value.js", "module.exports=42;"),
        ("common/explicit.mjs", "export const n=42;"),
        ("common/no-extension", "module.exports=42;"),
        ("module/package.json", "{\"type\":\"module\"}"),
        ("module/value.js", "export const n=42;"),
        ("module/explicit.cjs", "module.exports=42;"),
        ("module/no-extension", "export const n=42;"),
        ("module/unregistered.txt", "module.exports=42;"),
        ("unknown/package.json", "{\"type\":\"other\"}"),
        ("unknown/value.js", "export const n=42;"),
        ("node_modules/pkg/package.json", "{}"),
        ("node_modules/pkg/index.js", "export const n=42;"),
    ] {
        let path = directory.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let entry = directory.path().join("entry.cjs");
    std::fs::write(&entry, r#"
(async()=>{
const out=[];
for(const file of ['esm.js','meta.js','lexical.js','destructure.js','script.js','string.js','await-name.js','hashbang.js','bom.js','common/value.js','common/explicit.mjs','common/no-extension','module/value.js','module/explicit.cjs','module/no-extension','module/unregistered.txt','unknown/value.js','node_modules/pkg/index.js']){
 const value=require('./'+file);out.push([file,typeof value,typeof value==='object'?Object.keys(value):value,typeof value==='object'?value.n:null]);
}
out.push(['effects',detectedMeta.endsWith('/meta.js'),detectedLexical,detectedDestructure]);
out.push(['dynamic',(await require('./dynamic.js')()).n]);
try{require('./async.js');throw Error('unexpected success')}catch(e){out.push(['async',e.name,e.code,globalThis.detectedStarted===undefined]);}
await import('./async.js');out.push(['later',detectedStarted,detectedFinished]);
console.log(JSON.stringify({complete:true,out}));
})().catch(e=>{console.error(e.stack);process.exitCode=1});
"#).unwrap();
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable); command.arg(&entry);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(20)).unwrap()
    });
    for result in &observations {
        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(0), "{}", String::from_utf8_lossy(&result.stderr));
        assert!(result.stderr.is_empty(), "{}", String::from_utf8_lossy(&result.stderr));
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["complete"], true);
    }
    assert_eq!(String::from_utf8_lossy(&observations[0].stdout), String::from_utf8_lossy(&observations[1].stdout));
}

#[test]
fn stream_callable_duplex_transform_and_passthrough_initialize_supplied_receivers() {
    let observations = compare_node_stream_source(r#"
const stream=require('node:stream');
(async()=>{
const out=[];
for(const name of ['Duplex','Transform','PassThrough']){
 const C=stream[name];const parent=stream[name==='Duplex'?'Readable':name==='Transform'?'Duplex':'Transform'];
 function Derived() {this.returned=C.call(this,{read(){},write(c,e,cb){cb()},transform(c,e,cb){cb(null,c)}})}
 Object.setPrototypeOf(Derived.prototype,C.prototype);
 const d=new Derived();out.push([name,d instanceof C,!!d._readableState,!!d._writableState,d.returned===undefined,d.allowHalfOpen]);
 const plain=C({read(){},write(c,e,cb){cb()}});out.push(['factory',name,plain instanceof C]);
 class Child extends C {constructor(){super({read(){},write(c,e,cb){cb()}});this.marker=42}}
 const child=new Child();out.push(['subclass',name,child instanceof Child,child instanceof C,child.marker]);
 const target=Object.create(C.prototype);out.push(['call',name,C.call(target,{read(){},write(c,e,cb){cb()}})===undefined,!!target._readableState,!!target._writableState]);
 const closed=new C({readable:false,writable:false,allowHalfOpen:false});out.push(['disabled',name,closed.readableEnded,closed.writableFinished,closed.allowHalfOpen]);
 out.push(['metadata',name,C.name,C.length,Object.getPrototypeOf(C)===parent,C.prototype.constructor===C,Object.getOwnPropertyDescriptor(C,'prototype').writable]);
}
async function collect(value,input){const values=[];value.on('data',c=>values.push(c.toString()));const done=new Promise((resolve,reject)=>{value.on('error',reject);value.on('end',resolve)});value.end(input);await done;return values.join('')}
let transformed;let constructions=0;
function Upper(){stream.Transform.call(this,{construct(cb){constructions++;cb()},transform(c,e,cb){out.push(['transform receiver',this===transformed]);cb(null,c.toString().toUpperCase())},flush(cb){out.push(['flush receiver',this===transformed]);cb(null,'!')}})}
Object.setPrototypeOf(Upper.prototype,stream.Transform.prototype);transformed=new Upper();
out.push(['output',await collect(transformed,'hello'),constructions]);
function Through(){stream.PassThrough.call(this)}Object.setPrototypeOf(Through.prototype,stream.PassThrough.prototype);
out.push(['passthrough',await collect(new Through(),'data')]);
out.push(['custom pass',await collect(new stream.PassThrough({transform(c,e,cb){cb(null,'custom')}}),'data')]);
const chunk=Buffer.from('direct');let direct;
stream.PassThrough.prototype._transform.call({push(){throw Error('unexpected push')}},chunk,'buffer',(error,value)=>{direct=[error===null,value===chunk]});
out.push(['direct',direct,stream.PassThrough.prototype._transform.name,Object.hasOwn(stream.PassThrough.prototype._transform,'prototype')]);
console.log(JSON.stringify({complete:true,out}));
})().catch(e=>{console.error(e.stack);process.exitCode=1});
"#);
    for observation in observations {
        let result: serde_json::Value = serde_json::from_slice(&observation.stdout).unwrap();
        assert_eq!(result["complete"], true);
    }
}

#[test]
fn canonical_inspection_preserves_inherited_getter_receivers_cycles_and_depth() {
    compare_node_stream_source(r#"
const inspect=require('node:util').inspect,out=[];
class X { constructor(){this._y=123}get y(){return this._y} }
out.push(inspect(new X(),{getters:true,showHidden:true}));
class A { constructor(B){this.B=B}get b(){return this.B} }
class B { constructor(){this.A=new A(this)}get a(){return this.A} }
out.push(inspect(new B(),{depth:1,getters:true,showHidden:true}));
const value={get x(){throw 'getter failed'},get y(){return {nested:true}},set z(value){}};
for(const getters of [false,true,'get','set'])out.push(inspect(value,{getters,sorted:true,compact:false}));
const d=Object.getOwnPropertyDescriptor(inspect,'defaultOptions');out.push([d.enumerable,d.configurable,d.get.name,d.set.name]);
for(const options of [null,1,'x',Symbol('x'),()=>{}])try{inspect.defaultOptions=options;out.push('ok')}catch(e){out.push([e.name,e.code,e.message])}
console.log(JSON.stringify(out));
"#);
}

#[test]
fn canonical_inspection_unwraps_proxies_without_traps_and_keeps_binding_identity() {
    compare_node_source_with_args(r#"
const inspect=require('node:util').inspect,{getProxyDetails}=require('internal/test/binding').internalBinding('util'),out=[];
let calls=0;const target={x:1},handler={get(){calls++;throw Error('get trap')},ownKeys(){calls++;throw Error('keys trap')},getPrototypeOf(){calls++;throw Error('prototype trap')}};
const proxy=new Proxy(target,handler);
for(const flag of [undefined,true,false,1,'true',null]){
 const value=getProxyDetails(proxy,flag);out.push(flag===true?[value[0]===target,value[1]===handler]:value===target);
}
const details=getProxyDetails(proxy);out.push([details[0]===target,details[1]===handler]);
out.push(inspect(proxy),inspect(proxy,{showProxy:true}),calls);
const revoked=Proxy.revocable({},{});revoked.revoke();out.push(getProxyDetails(revoked.proxy),getProxyDetails(revoked.proxy,false),inspect(revoked.proxy),getProxyDetails({})===undefined);
const original=require('internal/util/inspect');out.push(original.inspect===inspect,original.format===require('node:util').format,original.formatWithOptions===require('node:util').formatWithOptions);
for(const v of [undefined,null,{},[],new Uint8Array(1),new Proxy({},{}),Symbol('External')])out.push(require('util').types.isExternal(v));
console.log(JSON.stringify(out));
"#,&["--expose-internals","--no-warnings"]);
}

#[test]
fn canonical_inspection_previews_collection_iterators_without_consuming_or_public_hooks() {
    compare_node_source_with_args(r#"
const inspect=require('node:util').inspect,{previewEntries}=require('internal/test/binding').internalBinding('util'),out=[];
const map=new Map([['a',1],['b',2],['c',3]]),set=new Set(['a','b','c']);
for(const value of [map,set,map.keys(),map.values(),map.entries(),set.values(),set.entries()]){
 if(typeof value.next==='function')value.next();
 const first=previewEntries(value),second=previewEntries(value,undefined);out.push([first,second,inspect(value)]);
 if(typeof value.next==='function')out.push(value.next());
}
const iterator=map.entries();iterator.next();map.delete('b');map.set('d',4);out.push(previewEntries(iterator),iterator.next());
map.clear();map.set('z',9);out.push(previewEntries(iterator),iterator.next(),iterator.next(),previewEntries(iterator,true));
const key={k:1},value={v:2},wm=new WeakMap([[key,value]]),ws=new WeakSet([key]);out.push(previewEntries(wm),previewEntries(ws),inspect(wm,{showHidden:true}),inspect(ws,{showHidden:true}));
for(const value of [undefined,null,{},[],[1].values(),new Proxy(map,{})])out.push(previewEntries(value,true)===undefined);
const original=Map.prototype.entries;Map.prototype.entries=()=>{throw Error('public entries')};try{out.push(previewEntries(map),inspect(map))}finally{Map.prototype.entries=original}
console.log(JSON.stringify(out));
"#,&["--expose-internals","--no-warnings"]);
}

#[test]
fn native_inspection_class_fallback_uses_slots_and_data_tags_without_public_getters() {
    compare_node_source_with_args(r#"
const {getConstructorName}=require('internal/test/binding').internalBinding('util'),out=[];let calls=0;
const plain=Object.create(null),tagged=Object.create(null),accessor=Object.create(null),ownConstructor={};
Object.defineProperty(tagged,Symbol.toStringTag,{value:'Tagged'});Object.defineProperty(accessor,Symbol.toStringTag,{get(){calls++;throw Error('tag getter')}});
Object.defineProperty(ownConstructor,'constructor',{get(){calls++;throw Error('constructor getter')}});
const array=[],map=new Map(),typed=new Uint16Array(1);for(const v of [array,map,typed])Object.setPrototypeOf(v,null);
const original=globalThis.Symbol;globalThis.Symbol=function(){throw Error('public Symbol')};
try{for(const v of [plain,tagged,accessor,ownConstructor,array,map,typed])out.push(getConstructorName(v))}finally{globalThis.Symbol=original}
out.push(calls);console.log(JSON.stringify(out));
"#,&["--expose-internals","--no-warnings"]);
}

#[test]
fn native_proxy_details_preserve_slot_identity_revocation_and_trap_isolation() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-proxy-details.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn native_collection_previews_keep_iterator_positions_live_slots_and_return_conventions() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-collection-preview.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cli_signal_defaults_reset_inherited_ignores_before_vm_workers_start() {
    use std::os::unix::process::CommandExt;
    let runner=std::path::PathBuf::from(env!("CARGO_BIN_EXE_run-parallel"));
    let cli=runner.with_file_name("quench-node");
    for (name,signal) in [("SIGINT",libc::SIGINT),("SIGQUIT",libc::SIGQUIT),("SIGTERM",libc::SIGTERM)] {
        for executable in [std::path::PathBuf::from("node"),runner.clone(),cli.clone()] {
            let mut command=Command::new(&executable);
            command.args(["-e","process.kill(process.pid,process.argv[1]);console.log('survived')",name]);
            if executable!=std::path::Path::new("node") { command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1"); }
            unsafe { command.pre_exec(move || {
                if libc::signal(signal,libc::SIG_IGN)==libc::SIG_ERR { return Err(std::io::Error::last_os_error()); }
                let limit=libc::rlimit{rlim_cur:0,rlim_max:0};
                if libc::setrlimit(libc::RLIMIT_CORE,&limit)!=0 { return Err(std::io::Error::last_os_error()); }
                Ok(())
            }); }
            let observation=quench_node_test::case_process::observe_command(command,Duration::from_secs(10)).unwrap();
            assert!(!observation.timed_out,"{executable:?} {name}");
            assert_eq!(observation.signal,Some(signal),"{executable:?} {name}: exit {:?}, stderr {}",observation.exit_code,String::from_utf8_lossy(&observation.stderr));
            assert_eq!(observation.exit_code,None);
            assert!(observation.stdout.is_empty());assert!(observation.stderr.is_empty());
        }
    }
}

#[test]
fn native_constructor_names_preserve_creation_provenance_and_prototype_roles() {
    compare_node_source_with_args(
        r#"
const {getConstructorName}=require('internal/test/binding').internalBinding('util'),{inspect}=require('node:util'),out=[];
function Alpha(){this.x=1}Object.defineProperty(Alpha,'name',{value:'Changed'});Alpha.prototype.constructor=null;
const a=new Alpha;out.push(getConstructorName(a));Object.setPrototypeOf(a,null);out.push(getConstructorName(a),inspect(a));
const bound=Alpha.bind(null);const b=new bound;Object.setPrototypeOf(b,null);out.push(getConstructorName(b));
function Beta(){}const reflected=Reflect.construct(Alpha,[],Beta);out.push(getConstructorName(reflected));Beta.prototype.constructor=null;out.push(getConstructorName(reflected));
const explicit={answer:42};function Returns(){return explicit};out.push(getConstructorName(new Returns));
const named={Computed:function(){this.y=2}};const c=new named.Computed;Object.setPrototypeOf(c,null);out.push(getConstructorName(c));
function Base(){}class Derived extends Base{}const d=new Derived;Derived.prototype.constructor=null;out.push(getConstructorName(d));
function Parent(){}const prototype=new Parent;Object.setPrototypeOf(prototype,null);out.push(getConstructorName(prototype));const child=Object.create(prototype);out.push(getConstructorName(prototype),getConstructorName(child));
console.log(JSON.stringify(out));
"#,
        &["--expose-internals", "--no-warnings"],
    );
}

#[test]
fn getter_stack_frames_preserve_original_names_and_inline_source_labels() {
    compare_node_source_with_args(
        r#"
const saved=Error.prepareStackTrace;Error.prepareStackTrace=(_,frames)=>frames.slice(0,2).map(f=>[f.getFunctionName(),f.getFileName()]);
const object={get x(){return new Error('getter').stack}},getter=Object.getOwnPropertyDescriptor(object,'x').get;Object.defineProperty(getter,'name',{value:'changed getter'});
function invoke(value){return value.x}try{console.log(JSON.stringify(invoke(object)))}finally{Error.prepareStackTrace=saved}
"#,
        &[],
    );
}

#[test]
fn native_constructor_names_use_safe_mutable_debug_names_and_original_prototype_names() {
    compare_node_source_with_args(
        r#"
const {getConstructorName}=require('internal/test/binding').internalBinding('util'),out=[];let calls=0;
for(const mode of ['empty','number','deleted','accessor','inherited']){
 function F(){};const value=new F;Object.setPrototypeOf(value,null);
 if(mode==='deleted')delete F.name;
 else if(mode==='accessor')Object.defineProperty(F,'name',{get(){calls++;throw Error('name getter')}});
 else if(mode==='inherited'){delete F.name;Object.setPrototypeOf(F,{name:'Inherited'})}
 else Object.defineProperty(F,'name',{value:mode==='empty'?'':17});
 out.push(getConstructorName(value));
 const child=Object.create({constructor:F});out.push(getConstructorName(child));
}
out.push(calls);console.log(JSON.stringify(out));
"#,
        &["--expose-internals", "--no-warnings"],
    );
}

#[test]
fn file_handle_read_parameters_preserve_defaults_options_getter_order_and_result_prototype() {
    compare_node_stream_source(include_str!("../node-tests/test-file-handle-read-parameters.js"));
}

#[test]
fn stack_function_names_follow_mutation_after_capture_and_preserve_utf16_without_getters() {
    compare_node_source_with_args(r#"
let calls=0;Error.prepareStackTrace=(_,frames)=>frames.slice(0,2);
function Original(){return new Error};const error=Original();Object.defineProperty(Original,'name',{value:'Changed'});
const sites=error.stack,out=[sites[0].getFunctionName()];
Object.defineProperty(Original,'name',{value:'Later'});out.push(sites[0].getFunctionName(),sites[0].toString());
Object.defineProperty(Original,'name',{value:'\ud800'});out.push(sites[0].getFunctionName());
Object.defineProperty(Original,'name',{get(){calls++;throw Error('name getter')}});out.push(sites[0].getFunctionName(),sites[1].getFunctionName()===null,calls);
Object.defineProperty(Original,'name',{value:'\0Public'});out.push(sites[0].getFunctionName());
Object.defineProperty(Original,'name',{value:''});out.push(sites[0].getFunctionName()===null);
console.log(JSON.stringify(out));
"#, &[]);
}

#[test]
fn native_constructed_names_preserve_base_identity_and_use_prototype_fallback_for_subclasses() {
    compare_node_source_with_args(
        r#"
const get=require('internal/test/binding').internalBinding('util').getConstructorName,out=[];
for(const [C,args] of [[Array,[]],[Map,[]],[Date,[]],[Uint8Array,[0]],[Error,[]],[TypeError,[]],[RangeError,[]],[URIError,[]],[EvalError,[]],[ReferenceError,[]],[SyntaxError,[]],[ArrayBuffer,[0]],[Number,[1]],[String,['x']],[Boolean,[true]],[Object,[]]]){
 const value=new C(...args),saved=Object.getOwnPropertyDescriptor(C,'name');Object.setPrototypeOf(value,null);
 try{Object.defineProperty(C,'name',{value:'Changed'});out.push(get(value))}finally{Object.defineProperty(C,'name',saved)}
}
function Original(){};const original=new Original;Object.setPrototypeOf(original,null);out.push(new Object(original)===original,get(new Object(original)));
class CustomArray extends Array{};const array=new CustomArray;out.push(get(array));CustomArray.prototype.constructor=null;out.push(get(array));
class CustomMap extends Map{};const map=new CustomMap;out.push(get(map));CustomMap.prototype.constructor=null;out.push(get(map));
const tagged=new CustomArray;Object.defineProperty(tagged,Symbol.toStringTag,{value:'TaggedArray'});out.push(get(tagged));
const foreign=require('vm').runInNewContext("const value=new Array;Object.setPrototypeOf(value,null);Object.defineProperty(Array,'name',{value:'ForeignArray'});value");out.push(get(foreign));
console.log(JSON.stringify(out));
"#,
        &["--expose-internals", "--no-warnings"],
    );
}

#[test]
fn canonical_util_types_preserve_native_sources_javascript_sources_export_order_and_identity() {
    compare_node_source_with_args(
        r#"
const types=require('util/types'),internal=require('internal/util/types'),raw=require('internal/test/binding').internalBinding('types');
const out=[types===internal,types===require('util').types,Object.keys(raw),Object.entries(types).map(([key,value])=>[key,value.name,value.length,Function.prototype.toString.call(value),Object.hasOwn(value,'prototype'),Object.getOwnPropertyDescriptor(types,key)])];
out.push(Object.keys(raw).every(key=>raw[key]===types[key]),types.isArrayBufferView===ArrayBuffer.isView);
console.log(JSON.stringify(out));
"#,
        &["--expose-internals", "--no-warnings"],
    );
}

#[test]
fn errors_and_stack_rendering_preserve_utf16_and_capture_targets_observe_original_getters() {
    compare_node_source_with_args(r#"
const out=[];for(const [C,args] of [[Error,[]],[TypeError,[]],[RangeError,[]],[AggregateError,[[]]],[SuppressedError,[1,2]]]){
 const error=new C(...args,'\ud800');out.push(error.message,error.toString(),error.stack.split('\n')[0]);
}
out.push(Error.prototype.toString.call({name:'\ud800',message:'\udfff'}));
for(const object of [{name:'Custom',message:'text'},{name:'',message:'text'},{},{get name(){throw 17},message:'text'}]){
 Error.captureStackTrace(object);try{out.push(object.stack.split('\n')[0])}catch(e){out.push(['threw',e])}
}
let calls=[];const target={get name(){calls.push('name');return {toString(){calls.push('name string');return '\ud800'}}},get message(){calls.push('message');return {toString(){calls.push('message string');return '\udfff'}}}};
Error.captureStackTrace(target);const first=target.stack;out.push(first.split('\n')[0],target.stack===first,calls);
Error.prepareStackTrace=(_,frames)=>frames[0].toString();function Frame(){return new Error};Object.defineProperty(Frame,'name',{value:'\ud800'});out.push(Frame().stack);
console.log(JSON.stringify(out));
"#, &[]);
}

#[test]
fn implemented_cli_option_snapshots_tokenization_and_stream_iterator_policy_match_node() {
    for flags in [&["--no-warnings","--expose-internals"][..], &["--no-warnings","--expose-internals","--experimental-stream-iter"][..]] {
        compare_node_source_with_args(include_str!("../node-tests/test-cli-options-stream-iter.js"),flags);
    }
}

#[test]
fn stream_iterator_webidl_uses_genuine_original_abort_signal_brands() {
    compare_node_source_with_args(include_str!("../node-tests/test-stream-iter-abort-brand.js"),&["--no-warnings","--expose-internals","--experimental-stream-iter"]);
}

#[test]
fn internal_abort_operation_uses_original_provider_state_reason_and_dependent_delivery() {
    compare_node_source_with_args(include_str!("../node-tests/test-internal-abort-provider.js"),&["--no-warnings","--expose-internals"]);
}

#[test]
fn text_decoder_stream_boundaries_and_repeated_flush_match_node() {
    compare_node_stream_source(r#"
const inputs=[['utf-8',[239,187,191,226,130,172,240,159,152,128,65]],['utf-16le',[255,254,61,216,0,222,65,0]],['utf-16be',[254,255,216,61,222,0,0,65]],['shift_jis',[130,160,65]],['big5',[164,164,65]],['euc-kr',[176,161,65]],['gb18030',[144,48,129,48,65]],['iso-2022-jp',[27,36,66,36,34,27,40,66,65]]];
const rows=[];
for(const [encoding,bytes]of inputs)for(const fatal of [false,true])for(const ignoreBOM of [false,true])for(let cut=0;cut<=bytes.length;cut++){
 const decoder=new TextDecoder(encoding,{fatal,ignoreBOM});
 try {const first=decoder.decode(Uint8Array.from(bytes.slice(0,cut)),{stream:true});const second=decoder.decode(Uint8Array.from(bytes.slice(cut)));const flushed=decoder.decode();rows.push([encoding,fatal,ignoreBOM,cut,[first,second,flushed]]);}
 catch(e){rows.push([encoding,fatal,ignoreBOM,cut,{name:e.name,code:e.code??null,message:e.message}]);}
}
console.log(JSON.stringify({complete:true,cases:rows.length,rows}));
"#);
}

#[test]
fn text_decoder_descriptors_validation_and_utf8_encoding_match_node() {
    compare_node_stream_source(r#"
const rows=[];
function capture(name,fn){try{rows.push({name,result:fn()});}catch(error){rows.push({name,error:{name:error.name,code:error.code??null,message:error.message}});}}
function descriptors(object){return Reflect.ownKeys(object).map(key=>{const d=Object.getOwnPropertyDescriptor(object,key);return {key:String(key),enumerable:d.enumerable,configurable:d.configurable,...('writable'in d?{writable:d.writable}:{}),...('value'in d?{value:typeof d.value==='function'?[d.value.name,d.value.length]:typeof d.value==='symbol'?String(d.value):d.value}:{}),...(d.get?{get:[d.get.name,d.get.length]}:{}),...(d.set?{set:[d.set.name,d.set.length]}:{})};});}
capture('decoder-prototype',()=>descriptors(TextDecoder.prototype));
capture('encoder-prototype',()=>descriptors(TextEncoder.prototype));
capture('decoder-instance-symbols',()=>Reflect.ownKeys(new TextDecoder()).map(String));
capture('encoder-instance-symbols',()=>Reflect.ownKeys(new TextEncoder()).map(String));
capture('decoder-label-symbol',()=>new TextDecoder(Symbol('encoding')).encoding);
for(const options of [null,3,true,'x',()=>{}])capture('decoder-options-'+String(options),()=>{const d=new TextDecoder('utf-8',options);return[d.encoding,d.fatal,d.ignoreBOM];});
capture('decoder-options-observation-order',()=>{const calls=[];const label={toString(){calls.push('label');return'utf-8';}};const options={get fatal(){calls.push('fatal');return true;},get ignoreBOM(){calls.push('ignoreBOM');return false;}};const d=new TextDecoder(label,options);return[calls,d.encoding,d.fatal,d.ignoreBOM];});
capture('decoder-label-versus-options-error-priority',()=>new TextDecoder('not-an-encoding',3).encoding);
for(const key of ['encoding','fatal','ignoreBOM'])capture('decoder-getter-brand-'+key,()=>Object.getOwnPropertyDescriptor(TextDecoder.prototype,key).get.call({}));
capture('decoder-decode-brand',()=>TextDecoder.prototype.decode.call({}));
for(const options of [null,3,true,'x',()=>{}])capture('decode-options-'+String(options),()=>new TextDecoder().decode(new Uint8Array([65]),options));
capture('decode-options-getter',()=>{const calls=[];const d=new TextDecoder();const out=d.decode(new Uint8Array([65]),{get stream(){calls.push('stream');return true;}});return[calls,out,d.decode()];});
capture('decode-shared-buffer',()=>{const b=new SharedArrayBuffer(2);new Uint8Array(b).set([65,66]);return new TextDecoder().decode(b);});
capture('decode-view-offset',()=>new TextDecoder().decode(new DataView(new Uint8Array([0,65,66,0]).buffer,1,2)));
capture('decode-detached-buffer',()=>{const b=new ArrayBuffer(2);structuredClone(b,{transfer:[b]});return new TextDecoder().decode(b);});
capture('encoder-symbol',()=>Array.from(new TextEncoder().encode(Symbol('text'))));
capture('encoder-encodeInto-invalid-destination',()=>new TextEncoder().encodeInto('x',[]));
capture('encoder-encodeInto-symbol',()=>new TextEncoder().encodeInto(Symbol('text'),new Uint8Array(30)));
capture('encoder-encodeInto-coercion-order',()=>{const calls=[];let error;try{new TextEncoder().encodeInto({toString(){calls.push('string');return'x';}},[]);}catch(e){error={name:e.name,code:e.code??null,message:e.message};}return[calls,error];});
capture('encoder-partial-codepoint',()=>{const out=[];for(let length=0;length<=5;length++){const b=new Uint8Array(length).fill(255);out.push([new TextEncoder().encodeInto('😀x',b),Array.from(b)]);}return out;});
console.log(JSON.stringify({complete:true,cases:rows.length,rows}));
"#);
}

#[test]
fn text_decoder_stream_errors_recover_and_reset_bom_match_node() {
    compare_node_stream_source(r#"
const fixtures=[
 ['utf-8',[0xef,0xbb,0xbf,0x41],[0xe2,0x28,0xa1]],
 ['utf-16le',[0xff,0xfe,0x41,0],[0,0xd8,0x41,0]],
 ['utf-16be',[0xfe,0xff,0,0x41],[0xd8,0,0,0x41]],
 ['shift_jis',[0x82,0xa0,0x41],[0x82,0x20,0xff]],
 ['big5',[0xa4,0xa4,0x41],[0xa4,0x20,0xff]],
 ['euc-kr',[0xb0,0xa1,0x41],[0xb0,0x20,0xff]],
 ['gb18030',[0x90,0x30,0x81,0x30,0x41],[0x90,0x30,0x20,0xff]],
 ['iso-2022-jp',[27,36,66,36,34,27,40,66,65],[27,36,255,27]],
 ['windows-1252',[0x41,0x80],[0x81,0xff]],
];
const rows=[];
for(const [encoding,valid,invalid]of fixtures)for(const fatal of [false,true])for(const ignoreBOM of [false,true])for(let cut=0;cut<=invalid.length;cut++){
 const decoder=new TextDecoder(encoding,{fatal,ignoreBOM}),out=[];
 const steps=[[invalid.slice(0,cut),true],[invalid.slice(cut),true],[[],false],[valid.slice(0,1),true],[valid.slice(1),false],[[],false],[valid,false]];
 for(const[bytes,stream]of steps)try{out.push({value:decoder.decode(Uint8Array.from(bytes),{stream})});}catch(e){out.push({error:{name:e.name,code:e.code??null,message:e.message}});}
 rows.push([encoding,fatal,ignoreBOM,cut,out]);
}
console.log(JSON.stringify({complete:true,cases:rows.length,calls:rows.length*7,rows}));
"#);
}

#[test]
fn callsites_share_native_prototypes_and_supported_methods_preserve_descriptors_and_brands() {
    compare_node_source_with_args(r#"
Error.prepareStackTrace=(_,frames)=>frames;function Frame(){return new Error};const first=Frame().stack,second=Frame().stack,site=first[0],proto=Object.getPrototypeOf(site),C=proto.constructor,out=[];
const methods=['getColumnNumber','getEvalOrigin','getFileName','getFunctionName','getLineNumber','getMethodName','getThis','getTypeName','isConstructor','isEval','isNative','toString'];
out.push(Reflect.ownKeys(site),Object.getPrototypeOf(first[1])===proto,Object.getPrototypeOf(second[0])===proto,C.name,C.length,Function.prototype.toString.call(C),Object.getOwnPropertyDescriptor(C,'prototype'),Object.getOwnPropertyDescriptor(proto,'constructor'));
for(const key of methods){const f=proto[key];out.push([key,f.name,f.length,Function.prototype.toString.call(f),Object.getOwnPropertyDescriptor(proto,key)]);}
for(const value of [undefined,null,1,'x',true,Symbol('s'),1n,{},()=>{},proto,new Proxy({}, {})])try{proto.getFileName.call(value)}catch(e){out.push([e.name,e.message])}
for(const invoke of [()=>C(),()=>new C])try{invoke()}catch(e){out.push([e.name,e.message])}
const get=require('internal/test/binding').internalBinding('util').getConstructorName;out.push(get(site));Object.defineProperty(C,'name',{value:'Changed'});proto.constructor=null;out.push(get(site));
console.log(JSON.stringify(out));
"#, &["--expose-internals","--no-warnings"]);
}

#[test]
fn async_iterator_protocol_and_secondary_realm_inheritance_match_node() {
    compare_node_stream_source(include_str!("../node-tests/test-async-iterator-protocol.js"));
}

#[test]
fn generator_loop_return_preserves_cleanup_order_rejections_and_finally() {
    compare_node_stream_source(include_str!("../node-tests/test-generator-loop-return.js"));
}

#[test]
fn async_delegated_return_unwinds_original_generator_finally() {
    compare_node_stream_source(include_str!("../node-tests/test-async-delegate-return.js"));
}

#[test]
fn callsite_function_identity_obeys_strict_frame_visibility_and_ignores_name_getters() {
    compare_node_source_with_args(r#"
Error.prepareStackTrace=(_,frames)=>frames;
function Loose(){return new Error}
function Strict(){'use strict';return Loose()}
function StrictLeaf(){'use strict';return new Error}
function Outer(){return StrictLeaf()}
const out=[];
for(const [invoke,expected] of [[Loose,[Loose]],[Strict,[Loose,Strict]],[Outer,[StrictLeaf,Outer]]]){
 const sites=invoke().stack;out.push(sites.slice(0,expected.length).map((site,index)=>[site.getFunction()===expected[index],site.getFunction()===undefined,site.getThis()===undefined]));
}
const site=Loose().stack[0];Object.defineProperty(Loose,'name',{get(){throw Error('name getter')}});out.push(site.getFunction()===Loose);
const method=Object.getPrototypeOf(site).getFunction;out.push(method.name,method.length,Function.prototype.toString.call(method),Object.getOwnPropertyDescriptor(Object.getPrototypeOf(site),'getFunction'));
for(const receiver of [{},undefined])try{method.call(receiver)}catch(e){out.push([e.name,e.message])}
console.log(JSON.stringify(out));
"#, &[]);
}

#[test]
fn callsite_method_names_follow_receiver_descriptors_without_getters_or_proxy_traps() {
    compare_node_source_with_args(
        r#"
Error.prepareStackTrace=(_,frames)=>frames[0];function Original(){return new Error().stack}
const out=[];
for(const object of [{Original},{renamed:Original},{a:Original,b:Original},Object.create({inherited:Original}),Object.defineProperty({},'hidden',{value:Original}),{0:Original},Object.defineProperty({},'Original',{value:Original})]){
 const key=Reflect.ownKeys(object)[0]??'inherited';const site=object[key]();out.push([key,site.getFunctionName(),site.getMethodName()]);
}
const strict={go:function Named(){'use strict';return new Error().stack}};out.push(strict.go().getMethodName());
const object={get value(){return new Error().stack}};out.push(object.value.getMethodName());
let calls=0;const proxy=new Proxy({alias:Original},{get(){calls++;throw Error('trap')},ownKeys(){calls++;throw Error('keys')},getPrototypeOf(){calls++;throw Error('prototype')}});out.push(Original.call(proxy).getMethodName(),calls);
const changing={old:Original};const site=changing.old();delete changing.old;changing.new=Original;out.push(site.getMethodName());
Object.defineProperty(Original,'name',{get(){calls++;throw Error('function name')}});out.push(site.getMethodName(),calls);
const wide={'\ud800':Original};out.push(wide['\ud800']().getMethodName());
const numeric={0(){return new Error().stack}};out.push(numeric[0]().getMethodName());
console.log(JSON.stringify(out));
"#,
        &[],
    );
}

#[test]
fn abort_listener_errors_are_reported_on_ticks_without_replacing_source_errors() {
    compare_node_source_with_args(include_str!("../node-tests/test-abort-listener-errors.js"), &["--no-warnings", "--experimental-stream-iter"]);
}

#[test]
fn function_factory_constructor_names_follow_original_realms_and_ignore_public_links() {
    compare_node_source_with_args(r#"
const get=require('internal/test/binding').internalBinding('util').getConstructorName,out=[];
const factories=[()=>function(){},()=>()=>{},()=>async function(){},()=>function*(){},()=>async function*(){}];
for(const factory of factories){
 const first=factory(),prototype=Object.getPrototypeOf(first),C=prototype.constructor;
 const name=Object.getOwnPropertyDescriptor(C,'name'),descriptor=Object.getOwnPropertyDescriptor(prototype,'constructor');
 Object.defineProperty(C,'name',{value:'Changed'+name.value});Object.defineProperty(prototype,'constructor',{value:null});
 const second=factory();out.push(Object.getPrototypeOf(second)===prototype);
 for(const value of [first,second]){Object.setPrototypeOf(value,null);out.push(get(value))}
 Object.defineProperty(C,'name',{value:''});out.push(get(first));
 let calls=0;Object.defineProperty(C,'name',{get(){calls++;throw Error('constructor name')}});out.push(get(first),calls);
 Object.defineProperty(prototype,'constructor',descriptor);Object.defineProperty(C,'name',name);
}
const prototypes=factories.map(factory=>Object.getPrototypeOf(factory()));
for(const name of ['AsyncFunction','GeneratorFunction','AsyncGeneratorFunction'])globalThis[name]={prototype:{fake:true}};
out.push(factories.map((factory,index)=>Object.getPrototypeOf(factory())===prototypes[index]));
for(const expression of ['function(){}','()=>{}','async function(){}','function*(){}','async function*(){}']){
 const values=require('vm').runInNewContext(`const make=()=>(${expression}),first=make(),p=Object.getPrototypeOf(first),C=p.constructor;Object.defineProperty(C,'name',{value:'Foreign'+C.name});Object.defineProperty(p,'constructor',{value:null});const second=make(),same=Object.getPrototypeOf(second)===p;Object.setPrototypeOf(first,null);Object.setPrototypeOf(second,null);[first,second,same]`);
 out.push(get(values[0]),get(values[1]),values[2]);
}
for(const value of [Array.prototype.map,Math.max,(()=>{}).bind(null),(async()=>{}).bind(null),(function*(){}).bind(null)]){
 const prototype=Object.getPrototypeOf(value),C=prototype.constructor,name=Object.getOwnPropertyDescriptor(C,'name');
 Object.setPrototypeOf(value,null);Object.defineProperty(C,'name',{value:'Changed'+name.value});
 try{out.push(get(value))}finally{Object.defineProperty(C,'name',name);Object.setPrototypeOf(value,prototype)}
}
console.log(JSON.stringify(out));
"#, &["--expose-internals","--no-warnings"]);
}

#[test]
fn constructor_class_names_preserve_iterator_origins_proxy_kinds_and_prototype_roles() {
    compare_node_source_with_args(include_str!("../node-tests/test-constructor-class-data.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn callsite_type_names_and_constructor_flags_follow_actual_activation_and_receiver_identity() {
    compare_node_source_with_args(include_str!("../node-tests/test-callsite-type-construction.js"), &[]);
}

#[test]
fn inferred_debug_names_preserve_empty_public_names_and_native_name_property_state() {
    compare_node_source_with_args(include_str!("../node-tests/test-inferred-function-debug-names.js"), &[]);
}

#[test]
fn native_zlib_streaming_counts_bytes_pending_close_and_iterable_pipelines_match_node() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-zlib-streaming.js"), &["--no-warnings", "--expose-internals", "--experimental-stream-iter"]);
}

#[test]
fn native_brotli_streaming_bytes_dictionaries_and_errors_match_node() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-brotli-streaming.js"), &["--no-warnings", "--expose-internals", "--experimental-stream-iter"]);
}

#[test]
fn native_zstd_streaming_frames_dictionaries_pledges_and_errors_match_node() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-zstd-streaming.js"), &["--no-warnings", "--expose-internals", "--experimental-stream-iter"]);
}

#[test]
fn default_node_stack_policy_uses_real_coded_brands_and_system_errors() {
    compare_node_source_with_args(include_str!("../node-tests/test-node-error-stack-policy.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn node_compatibility_version_follows_bundled_library_target() {
    compare_node_source_with_args(include_str!("../node-tests/test-node-compatibility-version.js"), &[]);
}

#[test]
fn native_buffer_backing_allocations_preserve_real_alignment_and_snapshots() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-buffer-allocation.js"),
        &["--no-warnings", "--expose-internals", "--expose-gc"],
    );
}

#[test]
fn native_buffer_pool_shares_real_aligned_backing_and_preserves_reentrant_copies() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-buffer-pool.js"),
        &["--no-warnings", "--experimental-stream-iter"],
    );
}

#[test]
fn boxed_primitive_brands_use_internal_values_and_reject_forged_properties_and_proxies() {
    compare_node_source_with_args(include_str!("../node-tests/test-boxed-primitive-slots.js"), &[]);
}

#[test]
fn native_buffer_unsafe_allocations_preserve_alignment_pooling_snapshots_and_errors() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-buffer-unsafe-alignment.js"),
        &["--no-warnings", "--expose-internals", "--expose-gc"],
    );
}

#[test]
fn native_buffer_concat_preserves_pooling_proxy_reads_reentrancy_and_padding() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-buffer-concat-pool.js"),
        &[],
    );
}

#[test]
fn native_buffer_latin1_classifies_actual_units_and_preserves_original_validation() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-buffer-latin1.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn native_buffer_public_policy_captures_original_validators_and_coded_constructors() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-native-buffer-policy-captures.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn callsite_columns_follow_member_parenthesized_and_constructor_call_syntax() {
    compare_node_source_with_args(include_str!("../node-tests/test-callsite-call-columns.js"), &[]);
}

#[test]
fn util_call_sites_capture_actual_metadata_without_error_hooks_and_use_node_validation() {
    compare_node_source_with_args(include_str!("../node-tests/test-util-real-call-sites.js"), &[]);
}

#[test]
fn shadow_realms_own_original_node_error_policy_default_hooks_and_private_fallbacks() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-shadow-stack-policy-isolation.js"),
        &["--no-warnings", "--expose-internals", "--experimental-shadow-realm", "--expose-gc"],
    );
}

#[test]
fn optional_private_and_complex_call_columns_follow_actual_syntax_tokens() {
    for source in [include_str!("../node-tests/test-optional-private-call-columns.js"), include_str!("../node-tests/test-call-column-trivia.js"), include_str!("../node-tests/test-complex-call-columns.js"), include_str!("../node-tests/test-special-call-columns.js")] {
        compare_node_source_with_args(source, &[]);
    }
}

#[test]
fn getter_operation_columns_remain_distinct_from_call_continuations() {
    compare_node_source_with_args(include_str!("../node-tests/test-getter-call-columns.js"), &[]);
}

#[test]
fn property_reads_and_writes_capture_their_own_operation_positions() {
    for source in [include_str!("../node-tests/test-property-access-columns.js"), include_str!("../node-tests/test-assignment-access-columns.js")] {
        compare_node_source_with_args(source, &[]);
    }
}

#[test]
fn source_positions_use_javascript_line_terminators_and_utf16_columns() {
    compare_node_source_with_args(include_str!("../node-tests/test-source-line-terminators.js"), &[]);
}

#[test]
fn fused_field_reads_preserve_each_getter_operation_source() {
    compare_node_source_with_args(include_str!("../node-tests/test-fused-field-source-positions.js"), &[]);
}

#[test]
fn callsite_eval_classification_follows_compilation_origin_and_survives_escaped_closures() {
    compare_node_source_with_args(include_str!("../node-tests/test-callsite-eval-origin.js"), &["--expose-gc"]);
}

#[test]
fn fs_glob_uses_original_policy_and_real_dirent_types_across_all_three_apis() {
    compare_node_source_with_args(include_str!("../node-tests/test-fs-original-glob-policy.js"), &[]);
}

#[test]
fn eval_uses_parsed_program_completion_grammar_and_original_source_positions() {
    compare_node_source_with_args(include_str!("../node-tests/test-compiled-eval-source.js"), &[]);

}

#[test]
fn fatal_source_excerpts_follow_javascript_line_terminators_and_original_file_lines() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source-lines.cjs");
    for separator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
        std::fs::write(&path, format!("void 0;{separator}  missing();")).unwrap();
        let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
            let mut command = Command::new(executable);
            command.arg(&path);
            if executable != "node" {
                command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
            }
            quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
        });
        for observation in &observations {
            assert!(!observation.timed_out);
            assert_eq!(observation.exit_code, Some(1));
            assert!(observation.stdout.is_empty());
        }
        let excerpts = observations.each_ref().map(|observation| {
            String::from_utf8_lossy(&observation.stderr).lines().take(4).map(str::to_owned).collect::<Vec<_>>()
        });
        assert_eq!(excerpts[0][1], "  missing();");
        assert_eq!(excerpts[0], excerpts[1]);
    }
}

#[test]
fn eval_call_sites_capture_original_compiling_callers_and_hide_absent_script_names() {
    compare_node_source_with_args(include_str!("../node-tests/test-callsite-eval-origins.js"), &["--expose-gc"]);
}

#[test]
fn indirect_eval_script_identity_preserves_origins_fresh_values_templates_and_realm_isolation() {
    compare_node_source_with_args(include_str!("../node-tests/test-indirect-eval-script-identity.js"), &[]);
}

#[test]
fn error_source_positions_preserve_captured_compiler_metadata_without_guest_hooks() {
    compare_node_source_with_args(include_str!("../node-tests/test-error-source-positions.js"), &["--expose-gc", "--expose-internals", "--no-warnings"]);
}

#[test]
fn native_protocol_constants_derive_from_configured_sdk_headers_and_preserve_descriptors() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-protocol-constants.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn native_caller_location_uses_original_activations_independently_of_error_hooks() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-caller-location.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn filesystem_glob_policy_defers_promises_dependencies_and_preserves_data_descriptors() {
    compare_node_source_with_args(include_str!("../node-tests/test-fs-promises-glob-lazy-policy.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn function_constructor_default_debug_names_remain_distinct_from_public_names_and_descriptors() {
    compare_node_source_with_args(include_str!("../node-tests/test-function-constructor-debug-names.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn function_prototype_info_lookups_preserve_native_accessor_layout_and_descriptor_transitions() {
    compare_node_source_with_args(include_str!("../node-tests/test-function-prototype-info-lookups.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn function_constructor_sources_preserve_whitespace_parameter_positions_and_synthetic_name_scope() {
    compare_node_source_with_args(include_str!("../node-tests/test-function-constructor-source-layout.js"), &[]);
}

#[test]
fn filesystem_readdir_sync_uses_sdk_policy_with_raw_names_and_recursive_native_layout() {
    compare_node_source_with_args(include_str!("../node-tests/test-fs-readdir-sdk-policy.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn private_fields_keep_class_creation_identity_and_atomic_internal_initialization() {
    compare_node_source_with_args(include_str!("../node-tests/test-private-field-identity.js"), &[]);
}

#[test]
fn private_eval_environments_cross_static_instance_and_ordinary_function_boundaries() {
    compare_node_source_with_args(include_str!("../node-tests/test-private-eval-environments.js"), &[]);
}

#[test]
fn private_brands_follow_initialization_without_branding_the_declaring_prototype() {
    compare_node_source_with_args(include_str!("../node-tests/test-private-brand-initialization.js"), &[]);
}

#[test]
fn static_blocks_retain_strict_eval_super_receiver_and_separate_var_contexts() {
    compare_node_source_with_args(include_str!("../node-tests/test-static-block-context.js"), &[]);
}

#[test]
fn tcp_default_listen_backlog_matches_node_when_syscall_tracing_is_available() {
    compare_node_source_with_args(include_str!("../node-tests/test-tcp-default-listen-backlog.js"), &[]);
}

#[test]
fn tcp_option_listen_backlog_matches_node_truthiness_and_int32_when_tracing_is_available() {
    compare_node_source_with_args(include_str!("../node-tests/test-tcp-option-listen-backlog.js"), &[]);
}

#[test]
fn utility_sdk_projection_preserves_mime_diff_timing_promise_state_and_uv_protocol_metadata() {
    compare_node_source_with_args(include_str!("../node-tests/test-util-sdk-projection.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn net_listen_lifecycle_retains_dns_generations_callbacks_and_public_method_shapes() {
    compare_node_source_with_args(include_str!("../node-tests/test-net-listen-lifecycle.js"), &["--expose-gc"]);
}

#[test]
fn net_dns_iterator_errors_close_once_preserve_original_abrupt_values_and_survive_gc() {
    compare_node_source_with_args(include_str!("../node-tests/test-net-dns-iterator-errors.js"), &["--expose-gc"]);
}

#[test]
fn process_capture_callback_reset_prioritizes_null_and_original_type_validation() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-callback-reset.js"), &[]);
}

#[test]
fn http_listening_derives_from_the_handle_and_close_cancels_pending_notification() {
    compare_node_source_with_args(include_str!("../node-tests/test-http-listening-handle.js"), &[]);
}

#[test]
fn process_capture_state_is_canonical_receiver_independent_and_survives_collection() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-canonical-state.js"), &["--expose-gc"]);
}

#[test]
fn process_uncaught_monitor_precedes_capture_and_can_replace_the_pending_handler() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-monitor-capture-transition.js"), &[]);
}

#[test]
fn process_uncaught_events_ignore_monitor_results_and_use_emit_truthiness() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-uncaught-emit-truthiness.js"), &[]);
}

#[test]
fn process_capture_delivery_reloads_the_callback_after_the_null_check() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-getter-order.js"), &[]);
}

#[test]
fn process_auxiliary_capture_runs_in_reverse_order_with_exact_true_handling() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-auxiliary.js"), &[]);
}

#[test]
fn process_auxiliary_capture_preserves_length_coercion_fractional_keys_and_replacement() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-auxiliary-coercion.js"), &["--expose-gc"]);
}

#[test]
fn process_capture_methods_have_public_names_and_ordinary_constructor_shapes() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-method-shape.js"), &[]);
}

#[test]
fn native_contextify_scripts_compile_once_keep_template_sites_and_return_global_functions() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-contextify-owned-script.js"), &["--expose-internals", "--no-warnings", "--expose-gc"]);
}

#[test]
fn sdk_mock_tracker_preserves_call_records_restoration_and_virtual_timer_queue() {
    compare_node_source_with_args(include_str!("../node-tests/test-sdk-mock-tracker.js"), &["--no-warnings"]);
}

#[test]
fn original_sdk_test_harness_runs_hooks_subtests_contexts_and_mock_cleanup() {
    compare_node_source_with_args(include_str!("../node-tests/test-sdk-test-harness.js"), &["--no-warnings", "--test-reporter=dot"]);
}

#[test]
fn promise_async_hooks_observe_real_resources_trigger_identity_and_job_order() {
    compare_node_source_with_args(include_str!("../node-tests/test-promise-async-hooks-lifecycle.js"), &[]);
}

#[test]
fn promise_hooks_keep_suspended_continuations_and_reactions_live_during_gc() {
    compare_node_source_with_args(include_str!("../node-tests/test-promise-hooks-gc-roots.js"), &["--expose-gc"]);
}

#[test]
fn promise_init_hooks_can_settle_await_sources_reentrantly() {
    compare_node_source_with_args(include_str!("../node-tests/test-promise-init-reentry.js"), &[]);
}

#[test]
fn fs_internal_module_stat_follows_links_returns_errno_and_resolves_node_cwd() {
    compare_node_source_with_args(include_str!("../node-tests/test-fs-internal-module-stat.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn process_fatal_handler_exits_preserve_sdk_status_event_suppression_and_explicit_requests() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-fatal-handler-exit.js"), &[]);
}


#[test]
fn cli_version_aliases_use_the_configured_sdk_and_short_circuit_before_guest_execution() {
    compare_node_source_with_args(include_str!("../node-tests/test-cli-version-order.js"), &[]);
}


#[test]
fn process_report_configuration_uses_sdk_descriptors_validation_and_shared_native_state() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-report-configuration.js"), &["--expose-internals", "--no-warnings"]);
}


#[test]
fn process_capture_preserves_report_flag_snapshot_restoration_and_accessor_order() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-capture-report-flags.js"), &[]);
}


#[test]
fn process_report_startup_flags_preserve_environment_precedence_aliases_and_signal_registration() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-report-startup.js"), &[]);
}


#[test]
fn process_events_share_emitter_storage_inspection_borrowed_methods_and_once_wrappers() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-shared-event-store.js"), &[]);
}

#[test]
fn module_cache_protocol_types_come_from_configured_sdk_and_preserve_native_descriptors() {
    compare_node_source_with_args(include_str!("../node-tests/test-module-cache-protocol.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn process_warning_policy_preserves_default_listener_removal_suppression_and_startup_descriptors() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-warning-policy.js"), &[]);
}

#[test]
fn sdk_test_context_survives_native_ticks_timers_and_immediate_callbacks() {
    compare_node_source_with_args(include_str!("../node-tests/test-sdk-test-context-callbacks.js"), &["--no-warnings", "--test-reporter=dot"]);
}

#[test]
fn native_callback_uncaught_handlers_observe_resource_before_after_hook_and_scope_restore() {
    compare_node_source_with_args(include_str!("../node-tests/test-async-callback-error-scope.js"), &[]);
}

#[test]
fn native_tick_resources_own_mutable_callback_slots_and_complete_actual_hook_lifecycle() {
    compare_node_source_with_args(include_str!("../node-tests/test-tick-resource-canonical-slots.js"), &[]);
}

#[test]
fn process_warning_uses_sdk_identity_and_only_public_option_getters() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-warning-sdk-authority.js"), &["--expose-internals", "--no-warnings"]);
}


#[test]
fn process_rejection_warnings_pass_public_arguments_and_preserve_original_stack() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-rejection-warning-arguments.js"), &["--unhandled-rejections=warn", "--no-warnings"]);
}


#[test]
fn process_report_native_compact_uses_truthiness_without_coercion_hooks() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-report-native-compact.js"), &["--expose-internals", "--no-warnings"]);
}

#[cfg(target_os = "linux")]

#[test]
fn process_native_cpu_resource_and_rss_facts_use_sdk_validation_and_live_os_data() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-native-metrics.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn process_binding_uses_sdk_metadata_native_identity_and_original_legacy_type_policy() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-binding-native.js"), &["--no-warnings"]);
}

#[test]
fn process_default_warning_listener_preserves_sdk_source_without_internal_exposure() {
    compare_node_source_with_args(include_str!("../node-tests/test-process-warning-listener-source.js"), &[]);
}

#[test]
fn process_binding_pending_warnings_follow_sdk_wrapper_native_once_state_and_flag_precedence() {
    let source = include_str!("../node-tests/test-process-binding-pending-warning.js");
    for flags in [vec![], vec!["--pending-deprecation"], vec!["--pending-deprecation", "--no-deprecation"], vec!["--pending-deprecation", "--no-pending-deprecation"]] {
        compare_node_source_with_args(source, &flags);
    }
}

#[test]
fn awaited_primitives_use_enclosing_async_promise_parent_for_real_hook_resources() {
    compare_node_source_with_args(include_str!("../node-tests/test-await-primitive-promise-parent.js"), &[]);
}

#[test]
fn missing_internal_bindings_throw_native_module_errors_with_original_descriptors() {
    compare_node_source_with_args(include_str!("../node-tests/test-internal-binding-missing-error.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn console_global_and_constructor_use_sdk_identity_namespace_and_output_policy() {
    compare_node_source_with_args(include_str!("../node-tests/test-console-sdk-authority.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn errors_reserve_native_stack_before_fields_and_capture_after_observable_conversions() {
    compare_node_source_with_args(include_str!("../node-tests/test-error-stack-layout.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn sdk_vfs_mount_lifecycle_and_native_reads_use_canonical_handler_state() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-read-bridge.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn sdk_vfs_handlers_preserve_receiver_argument_counts_results_and_errors() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-handler-protocol.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn private_promise_job_snapshots_preserve_public_async_ids_and_isolated_stores() {
    compare_node_source_with_args(include_str!("../node-tests/test-async-private-job-identities.js"), &["--no-warnings"]);
}

fn compare_node_file_source_with_args(source: &str, flags: &[&str]) {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("test.cjs");
    std::fs::write(&fixture, source).unwrap();
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(flags).arg(&fixture);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(), "{}", String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(observations[0].stdout, observations[1].stdout);
}

#[test]
fn sdk_vfs_mkdir_realpath_preserve_handler_protocol_and_result_projection() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-mkdir-realpath-protocol.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn sdk_vfs_open_preserves_flag_mode_validation_and_raw_path_dispatch() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-open-protocol.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn sdk_vfs_write_options_preserve_getter_order_identity_and_void_results() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-write-options-protocol.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn sdk_vfs_write_continuations_preserve_append_copying_loop_and_finally() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-write-continuation.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn sdk_vfs_write_native_effects_preserve_flags_bytes_and_descriptor_ownership() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-write-native-effects.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn sdk_vfs_write_fd_domains_and_append_delegation_match_source_policy() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-write-fd-domains.js"), &["--experimental-vfs", "--expose-internals", "--no-warnings"]);
}

#[test]
fn native_dns_lookup_policy_shares_promise_identity_defaults_and_numeric_bypass() {
    let source = include_str!("../node-tests/test-native-dns-public-policy.js");
    compare_node_source_with_args(source, &["--no-warnings"]);
    compare_node_source_with_args(source, &["--no-warnings", "--dns-result-order=ipv4first"]);
}

#[test]
fn native_dns_binding_dispatch_preserves_request_identity_and_async_submission_errors() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-dns-binding-dispatch.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn native_dns_requests_preserve_async_resource_storage_and_gc_lifetime() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-dns-request-lifetime.js"), &["--expose-gc", "--no-warnings"]);
}

#[test]
fn native_dns_lowlevel_completion_reads_current_callback_and_retains_request_receiver() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-dns-lowlevel-completion.js"), &["--expose-internals", "--expose-gc", "--no-warnings"]);
}

#[test]
fn native_dns_promise_requests_own_mutable_resolvers_and_preserve_binding_address_values() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-dns-promise-request-contract.js"), &["--expose-internals", "--no-warnings"]);
}

#[test]
fn native_tcp_constructor_bind_and_inherited_stream_prototypes_match_node() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-tcp-constructor.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_tcp_accept_preserves_handle_identity_and_real_close_completion() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-tcp-accept.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_stream_reads_and_synchronous_writes_project_real_buffers_and_state() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-tcp-stream.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_stream_pressure_completes_actual_write_request_once() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-tcp-async.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_stream_close_cancels_pending_write_before_handle_close() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-tcp-cancel.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_sdk_webcrypto_owns_public_namespace_and_real_import_generate_key_lifecycle() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-public-key-lifecycle.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn native_sdk_webcrypto_initializes_cold_global_and_preserves_random_policy() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-cold-global-random.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn native_sdk_dom_exception_and_quota_preserve_private_reflection_and_cause() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-dom-reflection.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn native_secret_key_job_preserves_hook_order_and_async_local_context() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-keygen-hooks-context.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn admitted_public_socket_owns_data_and_eof_after_native_read_request() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-public-socket-read-owner.js"), &["--no-warnings"]);
}


#[test]
fn native_tcp_close_callbacks_follow_real_libuv_lifo_requests() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-native-tcp-close-order.js"), &["--no-warnings", "--expose-internals"]);
}



#[test]
fn sdk_vfs_metadata_preserves_public_shape_options_accessors_and_error_boundaries() {
    let source = include_str!("../node-tests/test-sdk-vfs-metadata-protocol.js");
    compare_node_file_source_with_args(source, &["--no-warnings", "--expose-internals"]);
    compare_node_file_source_with_args(source, &["--no-warnings", "--expose-internals", "--experimental-vfs"]);
}

#[test]
fn sdk_vfs_virtual_metadata_and_descriptor_lifecycle_match_configured_source() {
    compare_node_file_source_with_args(include_str!("../node-tests/test-sdk-vfs-metadata-lifecycle.js"), &["--no-warnings", "--experimental-vfs"]);
}


#[test]
fn native_private_symbol_table_has_all_sdk_keys_and_private_semantics() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-private-symbol-table.js"), &["--no-warnings", "--expose-internals"]);
}

#[test]
fn native_exit_listener_accepts_owned_dns_and_crypto_work_without_callbacks() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-exit-work-normal.js"), &["--no-warnings"]);
}

#[test]
#[cfg(target_os = "linux")]
fn native_pool_reads_environment_at_first_dns_work_and_keeps_configuration() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-pool-first-use.js"), &["--no-warnings"]);
}

#[test]
fn native_pending_dns_and_crypto_keep_resources_and_als_across_gc() {
    compare_node_source_with_args(include_str!("../node-tests/test-native-pending-work-gc.js"), &["--no-warnings", "--expose-gc"]);
}

#[test]
fn native_exit_listener_throw_still_abandons_owned_work_without_callbacks() {
    let source = include_str!("../node-tests/test-native-exit-work-throw.js");
    for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
        let mut command = Command::new(executable);
        command.args(["--no-warnings", "-e", source]);
        if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
        let observation = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(1));
        assert_eq!(observation.stdout, b"native-exit-work-accepted\n");
        assert!(String::from_utf8_lossy(&observation.stderr).contains("Error: native-exit-lifecycle-marker"));
    }
}

#[test]
fn native_commonjs_realm_reuses_canonical_private_util_binding() {
    // This host bootstrap adapter is intentionally not a require-able Node module.
    let mut command = Command::new(env!("CARGO_BIN_EXE_run-parallel"));
    command.args(["--no-warnings", "--expose-internals", "-e", include_str!("../node-tests/test-native-realm-private-binding.js")]);
    command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
    let observation = quench_node_test::case_process::observe_command(command, Duration::from_secs(15)).unwrap();
    assert!(!observation.timed_out);
    assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
    assert!(observation.stderr.is_empty());
    assert_eq!(observation.stdout,b"native-realm-binding-identity-ok\n");
}


#[test]
fn child_process_original_sdk_mocktracker_intercepts_real_native_spawn() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-child-process-sdk-mocktracker-native-spawn.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn child_process_original_sdk_mocktracker_null_stdio_error_lifecycle() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-child-process-sdk-mocktracker-null-stdio-errors.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn child_process_original_sdk_mocktracker_null_stdio_timeout_lifecycle() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-child-process-sdk-mocktracker-null-stdio-timeout.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn child_process_direct_spawn_validates_in_sdk_order_and_closes_with_errno() {
    compare_node_source_with_args(
        include_str!("../node-tests/test-child-process-direct-spawn-validation-errors.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn crypto_original_sdk_hash_hmac_state_copy_and_errors() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-stateful-sdk-hash.js"),
        &["--no-warnings", "--expose-internals"],
    );
}


#[test]
fn crypto_original_sdk_hkdf_uses_native_provider_jobs_and_async_context() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-hkdf.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn abort_timeouts_use_original_sdk_weak_owners_and_listener_persistence() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-abort-timeout-native-weak-lifetime.js"),
        &["--no-warnings", "--expose-gc"],
    );
}


#[test]
fn crypto_native_jobs_resolve_legacy_provider_at_execution() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-digest-provider-transitions.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn crypto_original_sdk_pbkdf2_uses_provider_jobs_and_async_context() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-pbkdf2.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn crypto_original_sdk_key_family_owns_provider_keys_and_signature_jobs() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-key-family.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn crypto_native_sdk_domains_preserve_callback_resource_and_error_identity() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-crypto-native-sdk-domains.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn domain_sdk_owns_emitter_association_without_compatibility_global() {
    compare_node_file_source_with_args(
        include_str!("../node-tests/test-domain-source-emitter-policy.js"),
        &["--no-warnings", "--expose-internals"],
    );
}

#[test]
fn require_resolve_options_paths_resolve_paths_and_global_paths_match_node() {
    compare_node_stream_source(
        r#"
const fs=require('fs'),path=require('path'),os=require('os');
const root=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'resolve-paths-')));
fs.mkdirSync(path.join(root,'node_modules','pkg'),{recursive:true});
fs.writeFileSync(path.join(root,'node_modules','pkg','index.js'),'');
fs.writeFileSync(path.join(root,'local.js'),'');
const out=[],rel=p=>typeof p==='string'?p.replace(root,'<root>'):p;
const t=f=>{try{out.push(f())}catch(e){out.push([e.name,e.code,e.message.split('\n')[0].replace(root,'<root>')])}};
t(()=>rel(require.resolve('pkg',{paths:[root]})));
t(()=>rel(require.resolve('./local.js',{paths:[root]})));
t(()=>rel(require.resolve('pkg',{paths:[path.join(root,'missing'),root]})));
t(()=>rel(require.resolve('pkg/index.js',{paths:[path.join(root,'node_modules','pkg')]})));
t(()=>require.resolve('pkg',{paths:'x'}));
t(()=>require.resolve('pkg',{paths:[1]}));
t(()=>require.resolve('missing-pkg',{paths:[root]}));
t(()=>require.resolve('fs',{paths:[]}));
t(()=>require.resolve.paths('fs'));
t(()=>require.resolve.paths('node:fs'));
t(()=>[require.resolve.paths.name,require.resolve.paths.length,typeof require('module').createRequire(root+'/').resolve.paths]);
t(()=>require.resolve.paths('..x').length);
t(()=>require('module').globalPaths.slice(-1)[0].endsWith(path.join('lib','node')));
fs.rmSync(root,{recursive:true,force:true});
console.log(JSON.stringify(out));
"#,
    );
}

#[test]
fn advanced_ipc_serialization_carries_v8_values_like_node() {
    compare_node_stream_source(
        r#"
const {spawn}=require('child_process');
const child=`process.on('message',m=>{process.send({map:m.map instanceof Map&&m.map.get(1),self:m.self===m,big:m.big+1n,date:m.date instanceof Date,bytes:[...m.bytes]});process.disconnect()})`;
const c=spawn(process.execPath,['-e',child],{stdio:['inherit','inherit','inherit','ipc'],serialization:'advanced'});
const message={map:new Map([[1,'one']]),big:41n,date:new Date(0),bytes:new Uint8Array([1,2,3])};message.self=message;
c.on('message',reply=>console.log(JSON.stringify(reply,(k,v)=>typeof v==='bigint'?`${v}n`:v)));
c.on('exit',code=>console.log('exit',code));
c.send(message);
"#,
    );
}

#[test]
fn process_execve_replaces_the_process_and_reports_errors_like_node() {
    compare_node_source_with_args(
        r#"
const out = [];
const t = (f) => { try { out.push(f()); } catch (e) { out.push([e.name, e.code, e.message, e.errno, e.path, e.syscall, Object.keys(e)]); } };
t(() => [process.execve.length, process.execve.name, Object.getOwnPropertyDescriptor(process, 'execve').enumerable]);
t(() => process.execve('/nonexistent/quench-execve', ['x']));
t(() => process.execve('/bin/sh', [1]));
t(() => process.execve('/bin/sh', ['a\0b']));
t(() => process.execve('/bin/sh', [], { a: 1 }));
t(() => process.execve(1));
const seen = [];
require('diagnostics_channel').channel('process.execve').subscribe((m) => seen.push(m.execPath, m.args.length, m.env));
process.on('exit', () => console.log('exit handlers do not run'));
console.log(JSON.stringify(out));
process.execve('/bin/sh', ['sh', '-c', 'echo replaced "$X" "$#"', 'sh', 'one'], { X: 'kept' });
"#,
        &["--no-warnings"],
    );
}

/// The buffer/library ordering parity script: each case reads files before anything
/// touches `Buffer`, then observes the result.
const BUFFER_PARITY_SCRIPT: &str = r#"// Usage: node parity.cjs <case>. Each case reads files before touching Buffer, then observes.
const fs = require('fs');
const path = require('path');
const dir = __dirname;
const out = [];
const log = (...a) => out.push(a.map(v => typeof v === 'string' ? v : JSON.stringify(v)).join(' '));
const desc = (o, k) => { const d = Object.getOwnPropertyDescriptor(o, k); return d && ['value' in d ? 'data' : 'acc', d.enumerable, d.configurable, d.writable].join(','); };
const c = process.argv[2];
// Node's library loads internal/buffer.js without buffer.js for streams and encodings.
if (c === 'stream-first') require('stream');
const a = fs.readFileSync(path.join(dir, 'small.bin'));
const b = fs.readFileSync(path.join(dir, 'big.bin'));
const t = fs.readFileSync(path.join(dir, 'tiny.bin'));
const p = Object.getPrototypeOf(a);
if (c === 'keys') {
  log('ownKeys', Reflect.ownKeys(p).map(String));
  log('keys', Object.keys(p));
} else if (c === 'proto') {
  log(p === Object.getPrototypeOf(b), p === Object.getPrototypeOf(t), Object.getPrototypeOf(p) === Uint8Array.prototype);
  log(p === Buffer.prototype, a instanceof Buffer, a.constructor === Buffer);
} else if (c === 'ctor-first') {
  log(a.constructor === Buffer, a.constructor.name, p.constructor.name);
} else if (c === 'pool') {
  log(a.byteLength, a.buffer.byteLength, a.byteOffset % 8, b.buffer.byteLength, b.byteOffset, t.buffer.byteLength);
  log(a.buffer === t.buffer, (t.byteOffset - a.byteOffset));
  const x = Buffer.allocUnsafe(10);
  log(x.buffer === a.buffer, x.byteOffset - t.byteOffset);
  try { a.buffer.transfer(); log('transferred'); } catch (e) { log('transfer', e.name); }
} else if (c === 'inspect') {
  log(require('util').inspect(t).slice(0, 12));
  log(t.toString('hex').length, desc(p, 'parent'), desc(p, 'toString'), desc(p, 'constructor'));
} else if (c === 'desc-first') {
  log(desc(p, 'parent'), desc(p, 'offset'), desc(p, 'readUInt8'), desc(p, Symbol.for('nodejs.util.inspect.custom')));
  log(Reflect.ownKeys(p).length);
} else if (c === 'write-first') {
  p.toString = function () { return 'mine'; };
  log(a.toString(), Buffer.from('x').toString(), Object.keys(p).indexOf('toString'));
} else if (c === 'delete-first') {
  delete p.toString;
  log(typeof a.toString, Object.keys(p).includes('toString'), String(Object.getOwnPropertyNames(p).length));
} else if (c === 'define-first') {
  Object.defineProperty(p, 'swap16', { value: 1, writable: false, enumerable: false, configurable: true });
  log(desc(p, 'swap16'), Buffer.prototype.swap16, Object.keys(p).length);
} else if (c === 'buffer-first') {
  log(typeof Buffer.alloc, Buffer.prototype === p, Reflect.ownKeys(p).length, a.readUInt8(0) === a[0]);
} else if (c === 'parent') {
  log(a.parent === a.buffer, b.offset === b.byteOffset);
} else if (c === 'freeze') {
  Object.freeze(p);
  log(Object.isFrozen(p), typeof p.toString, Reflect.ownKeys(p).length);
} else if (c === 'forin') {
  const ks = []; for (const k in t) if (!/^\d+$/.test(k)) ks.push(k);
  log(ks.length, ks.slice(0, 3), ks.slice(-3));
} else if (c === 'stream-first') {
  log(typeof a.readUInt8, a.toString('hex', 0, 4), p === Buffer.prototype, Reflect.ownKeys(p).length);
} else if (c === 'sym') {
  const s = Symbol.for('nodejs.util.inspect.custom');
  log(typeof t[s], s in p, Object.getOwnPropertySymbols(p).map(String));
}
console.log(out.join('\n'));
"#;

fn buffer_parity_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("parity.cjs"), BUFFER_PARITY_SCRIPT).unwrap();
    // Below Node's unpooled size, above it, and within the heap-buffer size.
    let pattern = |length: u32, modulus: u32| (0..length).map(|i| (i % modulus) as u8).collect::<Vec<_>>();
    std::fs::write(directory.path().join("small.bin"), pattern(7684, 251)).unwrap();
    std::fs::write(directory.path().join("big.bin"), pattern(100_000, 241)).unwrap();
    std::fs::write(directory.path().join("tiny.bin"), pattern(50, 256)).unwrap();
    directory
}

fn run_quench(entry: &std::path::Path, args: &[&str]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_run-parallel"));
    command.arg(entry).args(args).env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
    let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(20)).unwrap();
    assert_eq!(result.exit_code, Some(0), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    String::from_utf8_lossy(&result.stdout).into_owned()
}

#[test]
fn buffer_prototype_keys_match_node() {
    // A prototype made before `buffer.js` loads ends with the keys the library gives it, in
    // the library's order and with its attributes, as one the library made itself.
    let directory = buffer_parity_directory();
    let probe = "const p = Object.getPrototypeOf(globalThis.__first());
      console.log(JSON.stringify(Reflect.ownKeys(p).map(k => [String(k), Object.getOwnPropertyDescriptor(p, k)]).map(([k, d]) => [k, 'value' in d, d.enumerable, d.configurable, d.writable])));";
    let eager = directory.path().join("eager.cjs");
    std::fs::write(&eager, format!("globalThis.__first = () => Buffer.alloc(1);\n{probe}")).unwrap();
    let lazy = directory.path().join("lazy.cjs");
    std::fs::write(&lazy, format!("globalThis.__first = () => require('fs').readFileSync(__dirname + '/small.bin');\n{probe}")).unwrap();
    let eager = run_quench(&eager, &[]);
    assert!(eager.contains("readBigUInt64LE") && eager.contains("nodejs.util.inspect.custom"), "{eager}");
    assert_eq!(run_quench(&lazy, &[]), eager);
}

#[test]
fn read_file_before_buffer_keeps_node_buffer_semantics() {
    // Expected outputs are Node v26.12.0's, the pinned library's release.
    let directory = buffer_parity_directory();
    let entry = directory.path().join("parity.cjs");
    for (case, expected) in [
        ("proto", r#"true true true\ntrue true true"#),
        ("ctor-first", r#"true Buffer Buffer"#),
        ("pool", r#"7684 65600 0 100000 0 65600\ntrue 7688\ntrue 56\ntransfer TypeError"#),
        ("inspect", r#"<Buffer 00 0\n100 acc,true,false, data,true,true,true data,false,true,true"#),
        ("desc-first", r#"acc,true,false, acc,true,false, data,true,true,true data,true,true,true\n97"#),
        ("write-first", r#"mine mine 79"#),
        ("delete-first", r#"function false 95"#),
        ("define-first", r#"data,false,true,false 1 94"#),
        ("buffer-first", r#"function true 97 true"#),
        ("parent", r#"true true"#),
        ("freeze", r#"true function 97"#),
        ("forin", r#"95 ["readBigUInt64LE","readBigUInt64BE","readBigUint64LE"] ["swap32","swap64","toLocaleString"]"#),
        ("sym", r#"function true ["Symbol(nodejs.util.inspect.custom)"]"#),
        ("stream-first", r#"function 00010203 true 97"#),
    ] {
        assert_eq!(run_quench(&entry, &[case]), format!("{}\n", expected.replace("\\n", "\n")), "{case}");
    }
}

#[test]
fn wasm_files_are_es_modules_as_in_node() {
    // Node's `wasm` translator: a `.wasm` file is a module whose namespace holds its
    // instance's exports (globals unwrapped), whose import modules are module specifiers,
    // and which instantiates when it evaluates; an entry `.wasm` runs the same way.
    let directory = tempfile::tempdir().unwrap();
    let wat = |name: &str, text: &str| {
        let buffer = wast::parser::ParseBuffer::new(text).unwrap();
        let mut module: wast::Wat = wast::parser::parse(&buffer).unwrap();
        std::fs::write(directory.path().join(name), module.encode().unwrap()).unwrap();
    };
    wat("lib.wasm", r#"(module (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add)
        (memory (export "mem") 1) (global (export "g") i32 (i32.const 42)) (global (export "m") (mut i32) (i32.const 7)))"#);
    wat("main.wasm", r#"(module (import "./lib.wasm" "g" (global i32)) (import "./lib.wasm" "add" (func $add (param i32 i32) (result i32)))
        (import "./host.mjs" "log" (func $log (param i32)))
        (func (export "start") global.get 0 i32.const 1 call $add call $log) (start 2))"#);
    wat("missing.wasm", r#"(module (import "./host.mjs" "nope" (func)))"#);
    wat("reserved-import.wasm", r#"(module (import "wasm-js:x" "f" (func)))"#);
    wat("reserved-export.wasm", r#"(module (func (export "wasm:f")))"#);
    std::fs::write(directory.path().join("bad.wasm"), b"\0asm\x01\0\0\0\x01\x05").unwrap();
    std::fs::write(directory.path().join("host.mjs"), "export function log(value) { console.log('log', value); }").unwrap();
    std::fs::write(directory.path().join("namespace.mjs"), "import * as lib from './lib.wasm';
        import { add, g, m } from './lib.wasm';
        console.log(Object.keys(lib), g, m, typeof add, add(2, 3), lib.mem instanceof WebAssembly.Memory,
          Object.prototype.toString.call(lib));
        const main = await import('./main.wasm');
        console.log(Object.keys(main), main === await import('./main.wasm'));
        for (const name of ['bad', 'missing', 'reserved-import', 'reserved-export']) {
          try { await import(`./${name}.wasm`); } catch (error) {
            const path = new URL(`./${name}.wasm`, import.meta.url);
            console.log(name, error.constructor.name, error.message.startsWith(`${path.pathname}: `),
              error.message.replace(path.href, '<url>'));
          }
        }").unwrap();
    // The warning goes through the public `process.emitWarning` as it is when it warns.
    std::fs::write(directory.path().join("patched.mjs"), "process.emitWarning = (message, type) => console.log(type);
        console.log((await import('./lib.wasm')).g);").unwrap();
    let warning = "ExperimentalWarning: Importing WebAssembly module instances is an experimental feature and might change at any time";
    for (entry, expected, warnings) in [
        (
            "namespace.mjs",
            "[ 'add', 'g', 'm', 'mem' ] 42 7 function 5 true [object Module]\nlog 43\n[ 'start' ] true\n",
            1,
        ),
        ("main.wasm", "log 43\n", 1),
        ("patched.mjs", "ExperimentalWarning\n42\n", 0),
    ] {
        for executable in ["node", env!("CARGO_BIN_EXE_run-parallel")] {
            let mut command = Command::new(executable);
            command.arg(directory.path().join(entry));
            if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
            let result = quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap();
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert_eq!(result.exit_code, Some(0), "{entry} {executable}: {stderr}");
            let errors: Vec<_> = stdout.lines().skip(expected.lines().count()).collect();
            assert!(stdout.starts_with(expected), "{entry} {executable}: {stdout}");
            assert_eq!(stderr.matches(warning).count(), warnings, "{entry} {executable}: {stderr}");
            if entry == "namespace.mjs" {
                assert_eq!(errors.len(), 4, "{executable}: {stdout}");
                assert!(errors[0].starts_with("bad CompileError true "), "{executable}: {}", errors[0]);
                assert!(errors[1].starts_with("missing LinkError false "), "{executable}: {}", errors[1]);
                assert_eq!(errors[2], "reserved-import LinkError false Invalid Wasm import \"wasm-js:x\" in <url>", "{executable}");
                assert_eq!(errors[3], "reserved-export LinkError false Invalid Wasm export name \"wasm:f\" in <url>", "{executable}");
            }
        }
    }
}

#[test]
fn process_warnings_match_node_without_loading_the_warning_module_graph() {
    // `process.emitWarning` and the default listener are Node's own functions over supplied
    // imports (`process_warning_shared_vm`): forms, validation errors, listener payloads,
    // option handling and the console the listener writes with match Node.
    let directory = tempfile::tempdir().unwrap();
    let scripts = [
        ("forms.mjs", r#"
const seen = [];
process.on('warning', (w) => seen.push([w.name, w.message, w.code, w.detail, w instanceof Error, w.stack.split('\n')[0]]));
const source = process.emitWarning.toString();
console.log(process.emitWarning.name, process.emitWarning.length, typeof process.emitWarning.prototype, source.slice(0, source.indexOf(')') + 1));
process.emitWarning('one');
process.emitWarning('two', 'CustomType', 'CODE1');
process.emitWarning('three', { type: 'T', code: 'C', detail: 'more' });
process.emitWarning(new Error('four'));
process.emitWarning('five', 'DeprecationWarning', 'DEP0');
function ctor() { process.emitWarning('six', 'X', undefined, ctor); }
ctor();
process.emitWarning('seven', '');
for (const args of [['eight', 5], [5]]) {
  try { process.emitWarning(...args); } catch (e) { console.log(e.code, e.message); }
}
setTimeout(() => console.log(JSON.stringify(seen)), 10);
"#),
        ("replaced.mjs", "console.error = (m) => console.log('replaced:', m); process.emitWarning('a');"),
        ("grouped.mjs", "console.group('g'); process.emitWarning('a'); setTimeout(() => { console.groupEnd(); process.emitWarning('b'); }, 5);"),
        ("late.mjs", "process.emitWarning('a'); setTimeout(() => { console.error = (m) => console.log('late:', m); process.emitWarning('b'); }, 5);"),
        ("custom.mjs", "const w = new Error('custom'); w.name = 'Mine'; w.toString = () => 'TOSTRING'; process.emitWarning(w); const v = new Error('z'); v.toString = 1; process.emitWarning(v); process.emitWarning('x', 'Mine');"),
    ];
    for (name, source) in scripts {
        std::fs::write(directory.path().join(name), source).unwrap();
    }
    let normalize = |text: &[u8]| {
        let text = String::from_utf8_lossy(text);
        let mut normalized = String::new();
        for line in text.lines() {
            // The process id, and the program name in the trace hint (`process.argv0`).
            let line = match (line.find("(node:"), line.find(") ")) {
                (Some(start), Some(end)) if start < end => format!("{}(node:PID{}", &line[..start], &line[end..]),
                _ => line.to_owned(),
            };
            let line = match (line.find("(Use `"), line.find(" --trace")) {
                (Some(start), Some(end)) if start < end => format!("(Use `PROGRAM{}", &line[end..]),
                _ => line,
            };
            normalized.push_str(&line);
            normalized.push('\n');
        }
        normalized
    };
    for flags in [&[][..], &["--no-warnings"], &["--disable-warning=CustomType", "--disable-warning=DeprecationWarning"], &["--no-deprecation"]] {
        for (name, _) in scripts {
            let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
                let mut command = Command::new(executable);
                command.args(flags).arg(directory.path().join(name));
                if executable != "node" { command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1"); }
                quench_node_test::case_process::observe_command(command, Duration::from_secs(10)).unwrap()
            });
            assert_eq!(observations[0].exit_code, observations[1].exit_code, "{name} {flags:?}");
            assert_eq!(normalize(&observations[0].stdout), normalize(&observations[1].stdout), "{name} {flags:?}");
            assert_eq!(normalize(&observations[0].stderr), normalize(&observations[1].stderr), "{name} {flags:?}");
        }
    }
}
