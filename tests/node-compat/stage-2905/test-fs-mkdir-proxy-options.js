'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

const dir = path.join(process.cwd(), 'tmp', 'node-compat-mkdir-proxy-options');
try { fs.rmSync(dir, { recursive: true }); } catch {}
fs.mkdirSync(path.join(process.cwd(), 'tmp'), { recursive: true });
const options = new Proxy({ mode: 0o755 }, {});
fs.mkdirSync(dir, options);
assert(fs.statSync(dir).isDirectory());
fs.rmSync(dir, { recursive: true });
