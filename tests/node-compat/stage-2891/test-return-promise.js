const assert = require('assert');
const { test } = require('node:test');

let finished = false;
test('returns a completion promise', () => {}).finally(() => { finished = true; });
setImmediate(() => assert.strictEqual(finished, true));
