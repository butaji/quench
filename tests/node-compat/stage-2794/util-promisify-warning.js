'use strict';

const assert = require('assert');
const { promisify } = require('util');
const seen = [];
process.on('warning', (warning) => seen.push(warning));
const wrapped = promisify(async (callback) => callback());
wrapped().then(() => {
  assert.strictEqual(seen.length, 1);
  assert.strictEqual(seen[0].code, 'DEP0174');
});
