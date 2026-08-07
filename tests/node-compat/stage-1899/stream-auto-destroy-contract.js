const assert = require("assert");
const { Readable, Writable, Transform } = require("stream");

(async () => {
  let readDestroy = 0;
  const readable = new Readable({
    autoDestroy: true,
    read() {
      this.push("hello");
      this.push(null);
    },
    destroy(error, callback) {
      readDestroy++;
      callback();
    },
  });
  const readEvents = [];
  readable.on("end", () => readEvents.push("end"));
  readable.on("close", () => readEvents.push("close"));
  readable.resume();
  await new Promise(setImmediate);
  assert.deepStrictEqual(readEvents, ["end", "close"]);
  assert.strictEqual(readDestroy, 1);

  let writeDestroy = 0;
  const writable = new Writable({
    autoDestroy: true,
    write(_chunk, _encoding, callback) {
      callback();
    },
    destroy(error, callback) {
      writeDestroy++;
      callback();
    },
  });
  const writeEvents = [];
  writable.on("finish", () => writeEvents.push("finish"));
  writable.on("close", () => writeEvents.push("close"));
  writable.end("hello");
  await new Promise(setImmediate);
  assert.deepStrictEqual(writeEvents, ["finish", "close"]);
  assert.strictEqual(writeDestroy, 1);

  let transformDestroy = 0;
  const transform = new Transform({
    autoDestroy: true,
    transform(chunk, _encoding, callback) {
      callback(null, chunk);
    },
    destroy(error, callback) {
      transformDestroy++;
      callback();
    },
  });
  const transformEvents = [];
  transform.on("end", () => transformEvents.push("end"));
  transform.on("finish", () => transformEvents.push("finish"));
  transform.on("close", () => transformEvents.push("close"));
  transform.resume();
  transform.end("hello");
  await new Promise(setImmediate);
  assert.deepStrictEqual(transformEvents, ["finish", "end", "close"]);
  assert.strictEqual(transformDestroy, 1);
})();
