import {createRequire} from 'node:module';
const require=createRequire('/workspace/quench/tests/npm-top300/package.json');
for(const name of ['punycode','tr46','json5','qs','ieee754','mime-types','string-width','json-buffer','minimist','path-key','eslint-visitor-keys','is-stream','statuses']) {
 const {exercise}=await import(`./${name}.mjs`);
 const unwrap=value=>value.default??value;
 console.log(JSON.stringify(exercise(unwrap(name==='string-width'?(await import('/workspace/quench/tests/npm-top300/node_modules/string-width/index.js')).default:require(name==='punycode'?'punycode/punycode.js':name)))));
}
console.log(JSON.stringify({complete:true}));
