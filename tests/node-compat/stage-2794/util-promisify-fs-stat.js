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
