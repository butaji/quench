'use strict';

const assert = require('node:assert');
const fs = require('node:fs');

const target = __filename;
fs.realpath(target, (error, resolved) => {
  assert.ifError(error);
  assert.strictEqual(resolved, fs.realpathSync(target));
});
