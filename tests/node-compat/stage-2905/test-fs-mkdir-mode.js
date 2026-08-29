'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

const dir = path.join(process.cwd(), 'tmp', 'node-compat-mkdir-mode');
try { fs.rmSync(dir, { recursive: true }); } catch {}
fs.mkdirSync(path.join(process.cwd(), 'tmp'), { recursive: true });
fs.mkdirSync(dir, 0o700);
assert(fs.statSync(dir).isDirectory());
fs.rmSync(dir, { recursive: true });
