const assert = require('assert');

const pairs = [];
for (const [index, value] of [[3, 7], [5, 11]]) {
  pairs.push(index + value);
}
assert.deepStrictEqual(pairs, [10, 16]);

const closures = [];
for (const [left, right] of [[1, 2], [3, 4]]) {
  closures.push(() => left + right);
}
assert.deepStrictEqual(closures.map((fn) => fn()), [3, 7]);
