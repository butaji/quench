const assert = require('assert');
const url = require('url');

assert.strictEqual(
  url.format({
    protocol: 'http:',
    hostname: 'example.com',
    pathname: '/a/b',
    query: { ok: 'yes' },
  }),
  'http://example.com/a/b?ok=yes',
);
assert.strictEqual(url.format('dot.test:foo/bar'), 'dot.test:foo/bar');
assert.strictEqual(
  url.format({ protocol: 'coap:', auth: 'u:p', hostname: '::1', port: '61616', pathname: '/r' }),
  'coap:u:p@[::1]:61616/r',
);
