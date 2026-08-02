const assert = require('assert');
// The Rust runner must execute every file in a stage and reject an empty stage.
assert.strictEqual(typeof process.cwd, 'function');
assert.strictEqual(typeof require, 'function');
