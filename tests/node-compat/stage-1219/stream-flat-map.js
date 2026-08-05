const assert = require("assert");
const { Readable } = require("stream");

Readable.from([1, 2])
  .flatMap((value) => [value, value])
  .toArray()
  .then((values) => assert.deepStrictEqual(values, [1, 1, 2, 2]));
