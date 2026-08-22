const assert = require('assert');
const tls = require('tls');

const context = tls.createSecureContext({
  sessionIdContext: 'sid',
  secureProtocol: 'TLS_method',
}).context;
assert.strictEqual(context.sessionIdContext, 'sid');
assert.strictEqual(context.secureProtocol, 'TLS_method');
console.log('tls context fields: ok');
