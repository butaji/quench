const assert = require('assert');
const tls = require('tls');

const ticketKeys = new Uint8Array(48);
const context = tls.createSecureContext({ ticketKeys }).context;
assert.strictEqual(context.ticketKeys.byteLength, 48);
console.log('tls ticket keys: ok');
