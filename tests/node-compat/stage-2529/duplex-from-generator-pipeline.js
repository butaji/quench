const assert = require("assert");
const { Duplex, pipeline } = require("stream");

pipeline(
  ["a\nb\n"],
  Duplex.from(async function* (source) {
    for await (const chunk of source) {
      yield chunk.toString().replaceAll("\n", "");
    }
  }),
  async function* (source) {
    let result = "";
    for await (const chunk of source) result += chunk;
    assert.strictEqual(result, "ab");
  },
  (error) => {
    if (error) throw error;
  },
);
