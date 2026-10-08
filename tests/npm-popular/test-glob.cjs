'use strict';

const assert = require('node:assert/strict');
const { globSync } = require('glob');

const matches = globSync('test-*.cjs', { cwd: __dirname });
assert.ok(matches.includes('test-glob.cjs'));
assert.ok(matches.includes('test-semver.cjs'));

