const assert = require('assert');
const tls = require('tls');

const context = tls.createSecureContext({
  ca: 'ca-data',
  cert: 'cert-data',
  key: 'key-data',
  passphrase: 'secret',
  ecdhCurve: 'auto',
}).context;
assert.strictEqual(context.ca, 'ca-data');
assert.strictEqual(context.cert, 'cert-data');
assert.strictEqual(context.key, 'key-data');
assert.strictEqual(context.passphrase, 'secret');
assert.strictEqual(context.ecdhCurve, 'auto');
console.log('tls context certificate options: ok');
