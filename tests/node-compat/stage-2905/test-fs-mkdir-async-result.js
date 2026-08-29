'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(process.cwd(), 'tmp', 'node-compat-mkdir-result');
try { fs.rmSync(root, { recursive: true }); } catch {}
fs.mkdir(path.join(root, 'child'), { recursive: true }, (error, first) => {
  assert.ifError(error);
  assert.strictEqual(first, root);
  fs.mkdir(path.join(root, 'child'), { recursive: true }, (error2, again) => {
    assert.ifError(error2);
    assert.strictEqual(again, undefined);
    fs.rmSync(root, { recursive: true });
  });
});
