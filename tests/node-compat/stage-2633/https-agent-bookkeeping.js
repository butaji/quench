const assert = require('assert');
const https = require('https');

const agent = new https.Agent();
assert.strictEqual(agent.maxFreeSockets, 256);
assert.deepStrictEqual(agent.freeSockets, {});
assert.deepStrictEqual(agent.sockets, {});
assert.deepStrictEqual(agent.requests, {});
assert.strictEqual(typeof agent.destroy, 'function');
console.log('https Agent bookkeeping: ok');
