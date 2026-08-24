const assert = require('assert');
const url = require('url');
const parsed = url.parse('/foo/bar/baz');
const resolved = parsed.resolveObject('quux');
assert.strictEqual(resolved.pathname, '/foo/bar/quux');
