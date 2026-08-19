// Node compat: repl + wasi shape.
const repl = require('node:repl');
const wasi = require('node:wasi');
if (typeof repl !== 'object') throw new Error('repl: ' + typeof repl);
if (typeof wasi !== 'object') throw new Error('wasi: ' + typeof wasi);
console.log('repl+wasi: ok');
