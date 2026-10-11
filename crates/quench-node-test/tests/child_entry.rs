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
