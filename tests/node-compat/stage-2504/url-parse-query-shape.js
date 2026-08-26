const assert = require('node:assert');
const url = require('node:url');

const parsed = url.parse('http://example.com', true);
assert.strictEqual(parsed.search, null);
assert.deepStrictEqual(Object.keys(parsed.query), []);
assert.strictEqual(Object.getPrototypeOf(parsed.query), null);
