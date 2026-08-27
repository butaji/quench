const assert = require('assert');
const { run } = require('node:test');

const stream = run({ files: [] });
assert.strictEqual(typeof stream.on, 'function');
