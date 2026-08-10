const assert = require("assert");
const { compose } = require("stream");

let output = "";
let consumed = "";
let ended = false;
let finished = false;
const keepAlive = setInterval(() => {}, 10);
const complete = () => {
  if (ended && finished) clearInterval(keepAlive);
};

compose(
  async function* (source) {
    for await (const chunk of source) yield chunk + chunk;
  },
  async function* (source) {
    for await (const chunk of source) yield chunk.toUpperCase();
  },
)
  .end("asd")
  .on("data", (chunk) => {
    output += chunk;
  })
  .on("end", () => {
    assert.strictEqual(output, "ASDASD");
    ended = true;
    complete();
  });

compose(
  async function* (source) {
    for await (const chunk of source) yield chunk.toUpperCase();
  },
  async function (source) {
    for await (const chunk of source) consumed += chunk;
  },
)
  .end("value")
  .on("finish", () => {
    assert.strictEqual(consumed, "VALUE");
    finished = true;
    complete();
  });

process.on("beforeExit", () => {
  assert.strictEqual(ended, true);
  assert.strictEqual(finished, true);
});
