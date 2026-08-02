const assert = require('assert');
const key = 'QUENCH_NODE_STAGE_58';
process.env[key] = 'value';
assert.strictEqual(process.env[key], 'value');
assert.strictEqual(key in process.env, true);
delete process.env[key];
assert.strictEqual(process.env[key], undefined);
