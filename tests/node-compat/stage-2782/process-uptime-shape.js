'use strict';
const assert = require('assert');
assert.strictEqual(typeof process.uptime, 'function');
const value = process.uptime();
assert.strictEqual(typeof value, 'number');
