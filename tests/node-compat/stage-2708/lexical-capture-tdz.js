'use strict';

const assert = require('assert');
const seen = [];

for (const encoding of ['utf8', 'ascii'].flatMap((value) => [value, value.toUpperCase()])) {
  seen.push(encoding);
}

['base64', 'hex'].forEach((encoding) => {
  assert.strictEqual(typeof encoding, 'string');
  seen.push(encoding);
});

assert.deepStrictEqual(seen, ['utf8', 'UTF8', 'ascii', 'ASCII', 'base64', 'hex']);
