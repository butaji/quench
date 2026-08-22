const assert = require('assert');
const https = require('https');

let error;
try {
  https.request('https://example.com');
} catch (caught) {
  error = caught;
}
assert.strictEqual(error.code, 'ERR_TLS_NOT_SUPPORTED');
assert.strictEqual(error.message, 'https.request is not supported by quench-node');
console.log('https error contract: ok');
