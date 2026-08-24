const assert = require('assert');
const url = require('url');
assert.strictEqual(
  url.resolve('http://asdf:qwer@www.example.com', 'http://diff:auth@www.example.com'),
  'http://diff:auth@www.example.com/',
);
assert.strictEqual(
  url.resolve('https://user:password@example.org/', 'https://another.host.com/'),
  'https://another.host.com/',
);
assert.strictEqual(
  url.resolve('https://example.com/foo', 'https://user:password@example.com'),
  'https://user:password@example.com/foo',
);
