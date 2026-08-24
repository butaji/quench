const assert = require('assert');
const url = require('url');

for (const [input, expected] of [
  ['/foo/bar?baz=quux#frag', { baz: 'quux' }],
  ['/example', {}],
  ['/example?query=value', { query: 'value' }],
]) {
  const parsed = url.parse(input, true);
  assert.strictEqual(Object.getPrototypeOf(parsed.query), null);
  assert.deepStrictEqual(Object.keys(parsed.query), Object.keys(expected));
  for (const key of Object.keys(expected)) assert.strictEqual(parsed.query[key], expected[key]);
}
