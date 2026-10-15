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
const original=fs.watch,hooks=[];
fs.watch=function(filename,options,listener){hooks.push(path.relative(dir,filename));return Reflect.apply(original,this,[filename,options,listener]);};
const watcher=original(dir,{recursive:true});fs.watch=original;
const result=[['paths',hooks],['meta',watcher.constructor.name,Object.keys(watcher),Object.getOwnPropertySymbols(Object.getPrototypeOf(watcher)).map(String),['close','ref','unref'].map(k=>[watcher[k].name,watcher[k].length,Object.hasOwn(watcher[k],'prototype')])]];
assert.strictEqual(watcher.ref(),undefined);assert.strictEqual(watcher.unref(),undefined);
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
fn console_formats_module_values_without_a_global_require() {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("entry.mjs");
    std::fs::write(&entry, r#"
import util from 'node:util';
console.log(null,undefined,42);
console.log('%s:%d:%j','answer',42,{value:null});
util.format=()=>{throw Error('public formatter replacement was used')};
util.formatWithOptions=util.format;
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
for(const bad of [undefined,null,{},1])assert.throws(()=>addAbortListener(bad,()=>{}),{code:'ERR_INVALID_ARG_TYPE'});
for(const bad of [undefined,null,{}])assert.throws(()=>addAbortListener(new AbortController().signal,bad),{code:'ERR_INVALID_ARG_TYPE'});
let options,removedCalls=0;const foreign={aborted:false,addEventListener(type,callback,value){options=value},removeEventListener(){removedCalls++}};
const foreignDisposable=addAbortListener(foreign,()=>{});foreignDisposable[Symbol.dispose]();foreignDisposable[Symbol.dispose]();
assert.strictEqual(Object.getPrototypeOf(options),null);assert(Object.isFrozen(options));assert.strictEqual(options.once,true);
result.push(removedCalls,Object.getOwnPropertySymbols(options).length);
queueMicrotask(()=>console.log(JSON.stringify(result)));
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
    for source in [source,trusted,rejected,mismatch,direct] {
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.args(["-e",&source]).env("QUENCH_TLS_PEER_ROOT",&fixtures);
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1").env("QUENCH_NODE_LIBRARY_ROOT",&fixtures);}
        quench_node_test::case_process::observe_command(command,Duration::from_secs(10)).unwrap()
    });
    assert!(!observations[0].timed_out,"Node TLS oracle timed out");
    assert!(!observations[1].timed_out,"native TLS client timed out: {}",String::from_utf8_lossy(&observations[1].stderr));
    assert_eq!(observations[0].exit_code,Some(0),"Node TLS oracle: {}",String::from_utf8_lossy(&observations[0].stderr));
    assert_eq!(observations[1].exit_code,Some(0),"native TLS client: {}",String::from_utf8_lossy(&observations[1].stderr));
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
    for (server_source, expected_client_exit) in [(server_source,0),(direct_server_source,0),(delayed_server_source.as_str(),0),(abrupt_server_source.as_str(),1)] {
    let observations=["node",env!("CARGO_BIN_EXE_run-parallel")].map(|executable|{
        let mut command=Command::new(executable);command.args(["-e",server_source]).env("QUENCH_TLS_PEER_ROOT",&fixtures).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if executable!="node"{command.env(quench_node::shared_run::CHILD_RUNNER_ENV,"1").env("QUENCH_NODE_LIBRARY_ROOT",&fixtures);}
        let mut peer=Peer(command.spawn().unwrap());let (mut stdout,line)=tls_peer_first_line(&mut peer.0);
        let port:u16=line.trim().parse().expect("TLS server did not start");
        let source=format!(r#"
const tls=require('tls'),fs=require('fs');const socket=tls.connect({{port:{port},host:'127.0.0.1',servername:'agent1',ca:fs.readFileSync(process.env.QUENCH_TLS_PEER_ROOT+'/test/fixtures/keys/ca1-cert.pem')}});let chunks=[];
socket.on('secureConnect',()=>socket.end(Buffer.from([0,22,23,127,254,255])));socket.on('data',chunk=>chunks.push(chunk));socket.on('end',()=>console.log(JSON.stringify([socket.authorized,Array.from(Buffer.concat(chunks))])));socket.on('error',e=>{{console.log(e.code,e.message);process.exitCode=1}});
"#);
        let mut client=Command::new("node");client.args(["-e",&source]).env("QUENCH_TLS_PEER_ROOT",&fixtures);
        let result=quench_node_test::case_process::observe_command(client,Duration::from_secs(10)).unwrap();
        assert!(!result.timed_out,"external client timed out");assert_eq!(result.exit_code,Some(expected_client_exit),"engine {executable}; server {server_source}; client stdout: {}; stderr: {}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
        let deadline=std::time::Instant::now()+Duration::from_secs(5);
        let status=loop{if let Some(status)=peer.0.try_wait().unwrap(){break status;}if std::time::Instant::now()>=deadline{let _=peer.0.kill();let _=peer.0.wait();panic!("TLS server did not close after peer EOF");}std::thread::sleep(Duration::from_millis(10));};
        let mut rest=String::new();stdout.read_to_string(&mut rest).unwrap();
        assert!(status.success(),"TLS server failed");
        (result.stdout,rest)
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
    compare_node_stream_source(r#"
'use strict';
const net=require('net'),tls=require('tls'),fs=require('fs');const output=[];
const root='/workspace/work/node-fixtures/test/fixtures/keys/';
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
"#);
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
console.log(JSON.stringify(out));
"#);
}
