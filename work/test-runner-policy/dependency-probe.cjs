'use strict';
// Diagnostic loader only: unchanged SDK source over existing host primitives.
const fs=require('fs'),path=require('path'),vm=require('vm');
const root='/workspace/work/node-stable-release/node-v26.12.0';
const primordials=require('internal/per_context/primordials');
const nativeBinding=require('internal/test/binding').internalBinding;
const cache=new Map();
const exitCodes={kNoFailure:0,kGenericUserError:1,kInternalJSParseError:3,kInternalJSEvaluationFailure:4,kV8FatalError:5,kInvalidFatalExceptionMonkeyPatching:6,kExceptionInFatalExceptionHandler:7,kInvalidCommandLineArgument:9,kBootstrapFailure:10,kInvalidCommandLineArgument2:12,kUnsettledTopLevelAwait:13,kStartupSnapshotFailure:14,kProcessTimeout:124,kAbort:134};
const binding=name=>{
 const real=nativeBinding(name);
 if(name==='errors')return {...real,exitCodes};
 if(name==='util')return {...real,getCallerLocation(){const frame=require('util').getCallSites(3)[2];return frame?[frame.lineNumber,frame.columnNumber,frame.scriptName]:[undefined,undefined,undefined]}};
 return real;
};
function load(id){
 if(id!=='test'&&!id.startsWith('internal/test_runner/'))return require(id);
 if(cache.has(id))return cache.get(id).exports;
 const filename=path.join(root,'lib',id+'.js');
 const module={exports:{}};cache.set(id,module);
 const source=fs.readFileSync(filename,'utf8');
 const factory=vm.runInThisContext('(function(exports,require,module,__filename,__dirname,primordials,internalBinding){\n'+source+'\n})',{filename,lineOffset:-1});
 factory(module.exports,load,module,filename,path.dirname(filename),primordials,binding);
 return module.exports;
}
const t=load('test');console.log('loaded',Object.keys(t));
