const assert = require('assert');

const source = Uint8Array.of(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
const destination = new Uint8Array(4);
Buffer.prototype.copy.call(source, destination, 1, 7, 10);
assert.deepStrictEqual([...destination], [0, 7, 8, 9]);
