import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
for(const name of ['punycode','tr46','json5','qs','ieee754','mime-types','string-width','json-buffer','minimist','path-key','eslint-visitor-keys','is-stream','statuses']) {
 const {exercise}=await import(`./${name}.mjs`);
 const actual=name==='punycode'?'punycode/punycode.js':name;
 const namespace=await import(actual),commonjs=require(actual);
 const results={import:exercise(namespace.default??namespace),require:exercise(commonjs.default??commonjs)};
 console.log(JSON.stringify({package:name,results}));
}
const {exercise}=await import('/workspace/scratch/quench-npm-file-url/tests/npm-top300/contracts/jackspeak.mjs');
const namespace=await import('jackspeak'),commonjs=require('jackspeak');
console.log(JSON.stringify({package:'jackspeak',results:{import:exercise(namespace),require:exercise(commonjs)}}));
console.log(JSON.stringify({complete:true}));
