'use strict';

const assert = require('node:assert/strict');
const ms = require('ms');

assert.strictEqual(ms('1.5h'), 5_400_000);
assert.strictEqual(ms('2 days'), 172_800_000);
assert.strictEqual(ms(90_000), '2m');
assert.strictEqual(ms(5_400_000), '2h');
