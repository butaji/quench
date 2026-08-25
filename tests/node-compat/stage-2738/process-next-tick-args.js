const assert = require('assert');
const expected = new Error('tick');
let actual;
process.nextTick((value) => { actual = value; }, expected);
process.nextTick(() => assert.strictEqual(actual, expected));
