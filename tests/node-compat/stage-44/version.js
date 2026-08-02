const assert = require('assert');
assert.match(process.version, /^v\d+\.\d+\.\d+$/);
assert.strictEqual(process.versions.node, '20.0.0');
assert.strictEqual(process.release.name, 'node');
assert.strictEqual(typeof process.versions.v8, 'string');
