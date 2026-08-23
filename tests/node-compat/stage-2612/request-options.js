const assert = require('assert');

const request = new Request('/resource', {
  method: 'POST',
  headers: { 'x-test': 'ok' },
  body: 'payload',
  credentials: 'include',
  redirect: 'manual',
  integrity: 'sha256-test',
});
assert.strictEqual(request.url, '/resource');
assert.strictEqual(request.method, 'POST');
assert.strictEqual(request.headers.get('x-test'), 'ok');
assert.strictEqual(request.credentials, 'include');
assert.strictEqual(request.redirect, 'manual');
assert.strictEqual(request.integrity, 'sha256-test');

assert.throws(() => new Request('/resource', { method: 'GET', body: 'payload' }), TypeError);
console.log('request options: ok');
