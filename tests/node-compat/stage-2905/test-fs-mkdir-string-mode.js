'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

const dir = path.join(process.cwd(), 'node-compat-mkdir-string-mode');
try { fs.rmSync(dir, { recursive: true }); } catch {}
fs.mkdirSync(dir, '10644');
assert.strictEqual(fs.statSync(dir).mode & 0o777, 0o644);
fs.rmSync(dir, { recursive: true });
