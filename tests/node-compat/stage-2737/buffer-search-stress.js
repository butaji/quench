const assert = require('assert');

const text = Array.from({ length: 65536 }, (_, index) => String.fromCharCode(index)).join('');
const buffer = Buffer.from(text);
assert.strictEqual(buffer.indexOf('notfound'), -1);
assert.strictEqual(buffer.indexOf('A'), 65);
