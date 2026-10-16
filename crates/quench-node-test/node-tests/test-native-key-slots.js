'use strict';
const assert = require('assert');
const crypto = require('crypto');
const { types, isDeepStrictEqual } = require('util');
for (const text of ['abc', 'xyz', '']) {
  const first = crypto.createSecretKey(Buffer.from(text));
  const second = crypto.createSecretKey(Buffer.from(text));
  const different = crypto.createSecretKey(Buffer.from(text + '!'));
  assert.strictEqual(types.isKeyObject(first), true);
  assert.strictEqual(types.isCryptoKey(first), false);
  assert.strictEqual(types.isKeyObject(Object.create(Object.getPrototypeOf(first))), false);
  assert.strictEqual(types.isKeyObject({ source: first, export() {} }), false);
  assert.strictEqual(isDeepStrictEqual(first, second), true);
  assert.strictEqual(isDeepStrictEqual(first, different), false);
  const copy = first.export();
  copy.fill(0);
  assert.strictEqual(first.export().toString('hex'), Buffer.from(text).toString('hex'));
}
for (const value of [null, undefined, 1, 'key', Symbol('key'), {}, []]) {
  assert.strictEqual(types.isKeyObject(value), false);
  assert.strictEqual(types.isCryptoKey(value), false);
}
console.log('native-key-slots-ok');
