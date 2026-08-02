const assert = require('assert');
const key = 'QUENCH_NODE_STAGE_59';
process.env[key] = 'value';
assert.strictEqual(Object.keys(process.env).includes(key), true);
delete process.env[key];
