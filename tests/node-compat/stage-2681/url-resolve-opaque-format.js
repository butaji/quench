const assert = require('assert');
const url = require('url');

const resolved = url.resolveObject(url.parse('http:base'), 'http:this');
assert.strictEqual(resolved.href, 'http:this');
assert.strictEqual(url.format(resolved), 'http:this');

const file = url.resolveObject(url.parse('file:/swap/test/animal.rdf'), '#Animal');
assert.strictEqual(file.href, 'file:/swap/test/animal.rdf#Animal');
assert.strictEqual(url.format(file), 'file:/swap/test/animal.rdf#Animal');
