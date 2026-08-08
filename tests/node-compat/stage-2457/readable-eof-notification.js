const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
const chunks = [];
let events = 0;

readable.on("readable", () => {
  events++;
  assert.strictEqual(readable._readableState.emittedReadable, true);
  const chunk = readable.read();
  if (chunk !== null) chunks.push(chunk.toString());
  assert.strictEqual(readable._readableState.emittedReadable, false);
});

process.nextTick(() => readable.push("foo"));
process.nextTick(() => readable.push("bar"));
setImmediate(() => {
  readable.push("quo");
  process.nextTick(() => readable.push(null));
});

process.on("beforeExit", () => {
  assert.deepStrictEqual(chunks, ["foo", "bar", "quo"]);
  assert.strictEqual(events, 3);
});
