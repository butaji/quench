'use strict';
const assert = require('assert');

process.env.QUENCH_NODE_ENV_COERCION = 42;
assert.strictEqual(process.env.QUENCH_NODE_ENV_COERCION, '42');
process.env.QUENCH_NODE_ENV_COERCION = true;
assert.strictEqual(process.env.QUENCH_NODE_ENV_COERCION, 'true');
delete process.env.QUENCH_NODE_ENV_COERCION;
