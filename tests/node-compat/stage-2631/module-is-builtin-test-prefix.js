const assert = require('assert');
const moduleApi = require('module');

assert.strictEqual(moduleApi.isBuiltin('test'), false);
assert.strictEqual(moduleApi.isBuiltin('node:test'), true);
assert.strictEqual(moduleApi.isBuiltin('node:sqlite'), true);
assert.strictEqual(moduleApi.isBuiltin('sqlite'), true);
console.log('module isBuiltin prefix semantics: ok');
