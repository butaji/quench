'use strict';

const assert = require('node:assert');
const fs = require('node:fs');

const file = 'node-compat-chmod-mode';
fs.writeFileSync(file, '');
fs.chmodSync(file, '444');
assert.strictEqual(fs.statSync(file).mode & 0o777, 0o444);
fs.unlinkSync(file);
