const assert = require("assert");
const { push, pull, tap, text } = require("stream/iter");

(async () => {
  const { writer, readable } = push();
  const seen = [];
  writer.write("hello");
  writer.end();
  const result = await text(pull(
    readable,
    tap(async (chunks) => {
      seen.push(chunks[0].toString());
    }),
  ));
  assert.strictEqual(result, "hello");
  assert.deepStrictEqual(seen, ["hello"]);
})();
