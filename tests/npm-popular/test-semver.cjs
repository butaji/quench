'use strict';

const assert = require('node:assert/strict');
const semver = require('semver');

assert.strictEqual(semver.valid('1.2.3'), '1.2.3');
assert.strictEqual(semver.compare('1.10.0', '1.9.9'), 1);
assert.strictEqual(semver.satisfies('7.8.5', '^7.0.0'), true);
assert.strictEqual(semver.satisfies('8.0.0', '^7.0.0'), false);
assert.strictEqual(semver.minVersion('^3.2.0').version, '3.2.0');

