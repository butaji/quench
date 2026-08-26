'use strict';
const common = require('../../node/test/common');
const assert = require('assert');
const { promisify } = require('util');

function values(callback) { callback(null, 'foo', 'bar'); }
promisify(values)().then(common.mustCall((value) => assert.deepStrictEqual(value, ['foo', 'bar'])));
function empty(callback) { callback(null); }
promisify(empty)().then(common.mustCall((value) => assert.strictEqual(value, undefined)));
function error(callback) { callback(new Error('oops')); }
promisify(error)().catch(common.mustCall((value) => assert.strictEqual(value.message, 'oops')));
function args(err, value, callback) { callback(err, value); }
promisify(args)(null, 42).then(common.mustCall((value) => assert.strictEqual(value, 42)));
const never = promisify(async () => {});
never();
