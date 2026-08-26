'use strict';

const assert = require('assert');
const { promisify } = require('util');
const fs = require('fs');

promisify(fs.stat)(__filename).then((value) => {
  assert.strictEqual(value.isFile(), true);
});
function fn() {}
function custom() {}
fn[promisify.custom] = custom;
assert.strictEqual(promisify(fn), custom);
const multi = (callback) => callback(null, 5, 17);
multi[Symbol.for('nodejs.util.promisify.customArgs')] = ['first', 'second'];
promisify(multi)().then((value) => {
  assert.deepStrictEqual(value, { first: 5, second: 17 });
});
const key = Symbol.for('nodejs.util.promisify.custom');
fn[key] = custom;
assert.strictEqual(promisify(fn), custom);
const object = {};
object.method = promisify(function (callback) { callback(null, this === object); });
object.method().then((value) => assert.strictEqual(value, true));
const error = new Error('oops');
const fail = promisify((callback) => callback(error));
fail().catch((value) => assert.strictEqual(value, error));
const throws = promisify(() => { throw error; });
throws().catch((value) => assert.strictEqual(value, error));
const asyncValue = promisify(async (callback) => callback(null, 42));
asyncValue().then((value) => assert.strictEqual(value, 42));
