const assert = require("assert");
const { Readable } = require("stream");

(async () => {
  const stream = new Readable({
    read() {
      setTimeout(() => {
        this.push("delayed");
        this.push(null);
      }, 0);
    },
  });
  const chunks = [];
  for await (const chunk of stream) chunks.push(chunk.toString());
  assert.deepStrictEqual(chunks, ["delayed"]);
})();
