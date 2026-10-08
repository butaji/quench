'use strict';

const assert = require('node:assert/strict');
const _ = require('lodash');

assert.deepStrictEqual(_.chunk([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]]);
assert.deepStrictEqual(_.merge({ flags: { a: true } }, { flags: { b: false } }), {
  flags: { a: true, b: false },
});
assert.strictEqual(_.get({ users: [{ name: 'Ada' }] }, 'users[0].name'), 'Ada');

