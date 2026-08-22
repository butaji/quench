const assert = require('assert');
const tls = require('tls');

const secureContext = tls.createSecureContext({
  minVersion: 'TLSv1.2',
  maxVersion: 'TLSv1.3',
  ciphers: 'DEFAULT',
});
assert.strictEqual(secureContext.context.minVersion, 'TLSv1.2');
assert.strictEqual(secureContext.context.maxVersion, 'TLSv1.3');
assert.strictEqual(secureContext.context.ciphers, 'DEFAULT');
console.log('tls context options: ok');
