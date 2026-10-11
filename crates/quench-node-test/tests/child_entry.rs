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
        '\x1b[99999mtext', '\x1b[31☃text', '\x1b]unterminated', '😀\x1b[0m😀'];
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

fn compare_node_stream_source(source: &str) {
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", source]);
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
