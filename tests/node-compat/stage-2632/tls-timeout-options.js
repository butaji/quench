const assert = require('assert');
const tls = require('tls');

const context = tls.createSecureContext({ handshakeTimeout: 10, sessionTimeout: 20 }).context;
assert.strictEqual(context.handshakeTimeout, 10);
assert.strictEqual(context.sessionTimeout, 20);
console.log('tls timeout options: ok');
