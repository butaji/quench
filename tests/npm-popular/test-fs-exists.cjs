'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const manifest = path.resolve('package.json');
assert.equal(fs.existsSync(manifest), true);
assert.equal(fs.existsSync(`${manifest}.missing`), false);
assert.throws(() => fs.exists(manifest), { code: 'ERR_INVALID_ARG_TYPE' });

let callbacks = 0;
fs.exists(manifest, (exists) => {
  assert.equal(exists, true);
  callbacks++;
});
fs.exists(`${manifest}.missing`, (exists) => {
  assert.equal(exists, false);
  callbacks++;
});
setImmediate(() => assert.equal(callbacks, 2));
