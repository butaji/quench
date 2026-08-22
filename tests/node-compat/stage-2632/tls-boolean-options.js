const assert = require('assert');
const tls = require('tls');

const context = tls.createSecureContext({ requestCert: true, rejectUnauthorized: false }).context;
assert.strictEqual(context.requestCert, true);
assert.strictEqual(context.rejectUnauthorized, false);
console.log('tls boolean options: ok');
