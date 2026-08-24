const assert = require('assert');
const url = require('url');
assert.strictEqual(
  url.resolve('http://asdf:qwer@www.example.com', 'http://diff:auth@www.example.com'),
  'http://diff:auth@www.example.com/',
);
