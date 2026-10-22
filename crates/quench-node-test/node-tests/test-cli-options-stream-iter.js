'use strict';
const options=require('internal/options'),m=require('module');
console.log('values',JSON.stringify(['--experimental-vfs','--no-experimental-vfs','--experimental-stream-iter','--no-experimental-stream-iter','--expose-internals','--no-expose-internals','--warnings','--no-warnings'].map(n=>[n,options.getOptionValue(n)])));
for(const text of ['', 'a b', '"" a', 'a\tb', "'a b'", 'a"b c"d', '"a\\"b"', '"a\\', '"a']){
 try{console.log('parse',JSON.stringify(text),JSON.stringify(options.parseNodeOptionsEnvVar(text)))}catch(e){console.log('parse-error',JSON.stringify(text),e.name,e.code,JSON.stringify(e.message))}
}
for(const specifier of ['stream/iter','node:stream/iter']){
 console.log('builtin',specifier,m.isBuiltin(specifier));
 try{const api=require(specifier);console.log('module',require.resolve(specifier),Object.keys(api).join(','),api===require('node:stream/iter'));console.log('sync',JSON.stringify(api.arraySync(['a','b'])),api.textSync(['a','b']))}catch(e){console.log('error',e.name,e.code,e.message)}
}
console.log('listed',m.builtinModules.filter(n=>n.startsWith('stream')).join(','));

const binding=require('internal/test/binding').internalBinding('options'),values=binding.getCLIOptionsValues();
console.log('value-metadata',Object.getPrototypeOf(values)===null,values.__proto__,JSON.stringify(Object.getOwnPropertyDescriptor(values,'--warnings')));
const before=options.getOptionValue('--experimental-stream-iter');process.env.NODE_OPTIONS='--experimental-stream-iter';process.execArgv.push('--experimental-stream-iter');options.refreshOptions();
console.log('snapshot',options.getOptionValue('--experimental-stream-iter')===before);
