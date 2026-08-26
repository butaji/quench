const assert = require("assert");

const nestedExpected = {};
nestedExpected.loop = nestedExpected;
nestedExpected.payload = { value: 1 };

const expected = {};
expected.loop = nestedExpected;
expected.payload = { value: 1 };

const actual = {};
actual.loop = expected;
actual.payload = { value: 1 };

assert.deepEqual(actual, expected);
assert.deepStrictEqual(actual, expected);
assert.partialDeepStrictEqual(actual, expected);
assert.deepEqual(expected, actual);
assert.deepStrictEqual(expected, actual);
assert.partialDeepStrictEqual(expected, actual);
