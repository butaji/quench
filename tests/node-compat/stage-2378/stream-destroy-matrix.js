const assert = require("assert");
const { Readable, Writable, destroy } = require("stream");

const cases = [
  [Readable, undefined, "AbortError"],
  [Readable, new Error("asd"), "Error"],
  [Writable, undefined, "AbortError"],
  [Writable, new Error("asd"), "Error"],
];

for (const [Constructor, error, expectedName] of cases) {
  const stream = new Constructor({
    read() {},
    write() {},
  });
  const events = [];
  stream.on("error", (actual) => {
    events.push("error");
    assert.strictEqual(actual.name, expectedName);
    if (error) assert.strictEqual(actual.message, "asd");
  });
  stream.on("close", () => events.push("close"));
  destroy(stream, error);
  assert.strictEqual(stream.destroyed, true);
  setTimeout(() => {
    assert.deepStrictEqual(events, ["error", "close"]);
    console.log("destroy case passed", expectedName);
  }, 0);
}
