const assert = require("assert");
const { Readable } = require("stream");

const controller = new AbortController();
controller.abort();
assert.rejects(
  Readable.from([1, 2, 3]).take(1, { signal: controller.signal }).toArray(),
  { name: "AbortError" },
);
