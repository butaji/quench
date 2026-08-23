const assert = require('assert');
const https = require('https');

const agent = new https.Agent({ keepAlive: true });
assert.strictEqual(agent.keepAlive, true);
assert.strictEqual(agent.protocol, 'https:');
assert.strictEqual(agent.defaultPort, 443);
assert.strictEqual(agent.options.keepAlive, true);
assert.strictEqual(https.globalAgent.protocol, 'https:');
console.log('https Agent constructor: ok');
