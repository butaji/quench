'use strict';

const assert = require('node:assert/strict');
const debug = require('debug');

process.env.DEBUG = 'quench:*';
debug.enable(process.env.DEBUG);
const log = debug('quench:smoke');
assert.strictEqual(log.enabled, true);
assert.strictEqual(debug.enabled('quench:smoke'), true);
assert.strictEqual(debug.enabled('other:smoke'), false);
debug.disable();
assert.strictEqual(debug.enabled('quench:smoke'), false);
