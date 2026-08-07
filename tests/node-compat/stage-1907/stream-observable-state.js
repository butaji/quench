const assert = require("assert");
const { Readable, Writable, isDisturbed, isErrored } = require("stream");

(async () => {
  const readable = new Readable({ read() {} });
  const readableState = readable._readableState;
  for (
    const name of [
      "reading",
      "ended",
      "endEmitted",
      "needReadable",
      "emittedReadable",
      "readableListening",
      "resumeScheduled",
      "readingMore",
      "dataEmitted",
      "errorEmitted",
    ]
  ) {
    assert.strictEqual(readableState[name], false, name);
  }
  assert.strictEqual(readable.readableDidRead, false);
  assert.strictEqual(isDisturbed(readable), false);
  assert.strictEqual(isErrored(readable), false);

  const readableEvents = [];
  readable.on("readable", () => {
    assert.strictEqual(readableState.readableListening, true);
    assert.strictEqual(readableState.emittedReadable, true);
    assert.strictEqual(readableState.needReadable, false);
    assert.ok(readable.read());
    assert.strictEqual(readableState.emittedReadable, false);
  });
  readable.on("end", () => readableEvents.push("end"));
  readable.on("close", () => readableEvents.push("close"));
  readable.push("state");
  readable.push(null);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepStrictEqual(readableEvents, ["end", "close"]);
  assert.strictEqual(readable.readableDidRead, true);
  assert.strictEqual(isDisturbed(readable), true);

  const flowing = new Readable({
    read() {
      this.push("data");
      this.push(null);
    },
  });
  const flowingEvents = [];
  flowing.on("data", () => {
    assert.strictEqual(flowing._readableState.resumeScheduled, false);
    assert.strictEqual(flowing._readableState.dataEmitted, true);
    flowingEvents.push("data");
  });
  flowing.on("end", () => flowingEvents.push("end"));
  flowing.on("close", () => flowingEvents.push("close"));
  assert.strictEqual(flowing._readableState.resumeScheduled, true);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepStrictEqual(flowingEvents, ["data", "end", "close"]);

  const writable = new Writable({
    write(_chunk, _encoding, callback) {
      assert.strictEqual(writable._writableState.ending, false);
      assert.strictEqual(writable._writableState.ended, false);
      assert.strictEqual(writable._writableState.finished, false);
      assert.strictEqual(writable._writableState.writable, undefined);
      callback();
    },
  });
  const writableEvents = [];
  writable.on("finish", () => {
    assert.strictEqual(writable._writableState.ending, true);
    assert.strictEqual(writable._writableState.ended, true);
    assert.strictEqual(writable._writableState.finished, true);
    writableEvents.push("finish");
  });
  writable.on("close", () => writableEvents.push("close"));
  assert.strictEqual(writable._writableState.ending, false);
  assert.strictEqual(writable._writableState.ended, false);
  assert.strictEqual(writable._writableState.finished, false);
  writable.end("state");
  assert.strictEqual(writable._writableState.ending, true);
  assert.strictEqual(writable._writableState.ended, true);
  assert.strictEqual(writable._writableState.finished, false);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepStrictEqual(writableEvents, ["finish", "close"]);
})();
