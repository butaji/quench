// npm — the real published `ms` package (vendored, MIT) loads and runs
// under quench-node via node_modules resolution, with output matching Node.
'use strict';
const assert = require('assert');
const ms = require('ms');

assert.strictEqual(typeof ms, 'function', 'ms exports a function');
assert.strictEqual(ms('2 days'), 172800000, 'parse duration');
assert.strictEqual(ms(1000), '1s', 'format milliseconds');
assert.strictEqual(ms(60000), '1m', 'format minutes');

console.log('npm-ms: ok');