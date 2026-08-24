const assert = require('assert');
const url = require('url');

const parsed = url.parse('http://user%3Apw@example.com/a?b=c#d');
assert.strictEqual(parsed.protocol, 'http:');
assert.strictEqual(parsed.auth, 'user:pw');
assert.strictEqual(parsed.pathname, '/a');
assert.strictEqual(parsed.query, 'b=c');
assert.strictEqual(parsed.href, 'http://user:pw@example.com/a?b=c#d');
