'use strict';

const assert = require('node:assert/strict');
const glob = require('glob');

const matches = glob.sync('test-*.cjs', { cwd: __dirname });
assert.ok(matches.includes('test-glob.cjs'));
assert.ok(matches.includes('test-semver.cjs'));

