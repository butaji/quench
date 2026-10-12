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
