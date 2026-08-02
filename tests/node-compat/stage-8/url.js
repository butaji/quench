const assert = require('assert');
const { URL, URLSearchParams } = globalThis;
const url = new URL('https://example.test/a?x=1&x=2#frag');
assert.strictEqual(url.protocol, 'https:');
assert.strictEqual(url.hostname, undefined); // hostname remains an explicit future URL stage
assert.strictEqual(url.searchParams.getAll('x').length, 2);
url.searchParams.set('x', '3');
assert.strictEqual(url.searchParams.toString(), 'x=3');
const params = new URLSearchParams({ hello: 'world' });
assert.strictEqual(params.get('hello'), 'world');
