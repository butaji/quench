const assert = require("assert");
const { merge, text, toStreamable, toAsyncStreamable } = require("stream/iter");

(async () => {
  const streamable = { [toStreamable]: () => "ghi" };
  const asyncStreamable = { [toAsyncStreamable]: () => Promise.resolve("jkl") };
  assert.strictEqual(await text(merge(streamable)), "ghi");
  assert.strictEqual(await text(merge(asyncStreamable)), "jkl");
})();
