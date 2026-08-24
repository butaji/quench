'use strict';

const assert = require('assert');

const util = require('util');
const types = require('util/types');

assert.strictEqual(types, util.types);
assert.strictEqual(typeof types.isNativeError, 'function');
assert.strictEqual(types.isNativeError(new Error('x')), true);
