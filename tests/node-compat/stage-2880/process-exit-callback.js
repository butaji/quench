const assert = require('assert');
const process = require('process');
let called = false;
process.on('exit', () => { called = true; });
assert.strictEqual(typeof process.on, 'function');
