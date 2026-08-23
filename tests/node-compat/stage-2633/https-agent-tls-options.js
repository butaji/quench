const assert = require('assert');
const https = require('https');

assert.strictEqual(new https.Agent().rejectUnauthorized, true);
assert.strictEqual(new https.Agent({ rejectUnauthorized: false }).rejectUnauthorized, false);
console.log('https Agent TLS options: ok');
assert.strictEqual(new https.Agent().scheduling, 'lifo');
assert.strictEqual(new https.Agent({ scheduling: 'fifo' }).scheduling, 'fifo');
