const assert = require('assert');
assert.strictEqual(typeof process.config.variables, 'object');
assert.strictEqual(typeof process.features.inspector, 'boolean');
assert.strictEqual(process.config.variables.node_shared, false);
