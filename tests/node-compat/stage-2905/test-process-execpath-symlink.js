'use strict';

const assert = require('node:assert');
const childProcess = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const link = path.join(process.cwd(), 'tmp', 'node-compat-exec-link');
fs.mkdirSync(path.join(process.cwd(), 'tmp'), { recursive: true });
try { fs.unlinkSync(link); } catch {}
fs.symlinkSync(process.execPath, link);
const result = childProcess.spawnSync(link, [__filename, 'child']);
assert.strictEqual(result.status, 0);
assert.strictEqual(result.stderr.toString(), '');
assert.strictEqual(result.stdout.toString(), `${process.execPath}\n`);
fs.unlinkSync(link);
