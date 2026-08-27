'use strict';

const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

const dir = path.join(process.cwd(), 'node-compat-mkdir-proxy-options');
try { fs.rmSync(dir, { recursive: true }); } catch {}
const options = new Proxy({ mode: 0o755 }, {});
fs.mkdirSync(dir, options);
assert(fs.statSync(dir).isDirectory());
fs.rmSync(dir, { recursive: true });
