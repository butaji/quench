const assert = require('assert');
const https = require('https');

assert.strictEqual(new https.Agent().maxSockets, Infinity);
assert.strictEqual(new https.Agent({ maxSockets: 4 }).maxSockets, 4);
console.log('https Agent defaults: ok');
