'use strict';

const assert = require('node:assert/strict');
const { minimatch } = require('minimatch');

assert.strictEqual(minimatch('src/index.js', 'src/**/*.js'), true);
assert.strictEqual(minimatch('src/index.test.js', 'src/!(*.test).js'), false);
assert.strictEqual(minimatch('README.md', '*.{md,txt}'), true);
assert.deepStrictEqual(minimatch('a/b/c.js', '**/*.js', { matchBase: true }), true);
