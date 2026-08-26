const assert = require('node:assert');
const url = require('url');

const parsed = url.parse('/example?query=value', true);
assert.strictEqual(parsed.search, '?query=value');
assert.deepStrictEqual(Object.keys(parsed.query), ['query']);
assert.strictEqual(parsed.query.query, 'value');
assert.strictEqual(Object.getPrototypeOf(parsed.query), null);
