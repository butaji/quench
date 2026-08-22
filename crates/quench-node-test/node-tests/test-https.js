// HTTPS request surface remains callable in sole QuenchRuntime.
const assert = require('node:assert');
const https = require('node:https');
assert.strictEqual(typeof https.request, 'function');
assert.strictEqual(typeof https.get, 'function');
console.log('https: ok');
