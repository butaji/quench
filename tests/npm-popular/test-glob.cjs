'use strict';

const assert = require('node:assert/strict');
const glob = require('glob');
const fs = require('node:fs');
const fsPromises = require('node:fs/promises');

assert.strictEqual(fsPromises, fs.promises);
assert.strictEqual(fsPromises.constants, fs.constants);
assert.strictEqual(typeof fs.constants.O_RDONLY, 'number');
assert.strictEqual(Object.getPrototypeOf(fs.constants), null);

const matches = glob.sync('test-*.cjs', { cwd: __dirname });
assert.ok(matches.includes('test-glob.cjs'));
assert.ok(matches.includes('test-semver.cjs'));

