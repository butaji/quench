'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const manifest = path.resolve('package.json');
assert.equal(fs.existsSync(manifest), true);
assert.equal(fs.existsSync(`${manifest}.missing`), false);
assert.throws(() => fs.exists(manifest), { code: 'ERR_INVALID_ARG_TYPE' });
assert.equal(fs.accessSync(manifest), undefined);
assert.throws(() => fs.accessSync(`${manifest}.missing`), { code: 'ENOENT' });

let callbacks = 0;
fs.exists(manifest, (exists) => {
  assert.equal(exists, true);
  callbacks++;
});
fs.exists(`${manifest}.missing`, (exists) => {
  assert.equal(exists, false);
  callbacks++;
});
fs.access(manifest, (error) => {
  assert.ifError(error);
  callbacks++;
});
fs.promises.access(manifest).then(() => {
  callbacks++;
});
setImmediate(() => assert.equal(callbacks, 4));
