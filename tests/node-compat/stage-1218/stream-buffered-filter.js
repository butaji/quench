const assert = require("assert");
const { Readable } = require("stream");

Readable.from([1, 2, 3])
  .filter((value) => value < 3)
  .toArray()
  .then((values) => assert.deepStrictEqual(values, [1, 2]));
