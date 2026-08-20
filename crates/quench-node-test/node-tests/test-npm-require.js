// npm — bare-specifier require resolves through node_modules and the
// package.json `main` field, loads the file as CJS, and returns its
// exports (function + attached property).
'use strict';
const assert = require('assert');
const add = require('quench-fixture');

assert.strictEqual(typeof add, 'function', 'package main exported a function');
assert.strictEqual(add(2, 3), 5, 'function body runs');
assert.strictEqual(add.desc, 'quench-fixture', 'attached property exported');
assert.strictEqual(add.depLabel, 'quench-dep', 'nested dependency resolved');

console.log('npm-require: ok');