const assert = require("assert");
const { Duplex } = require("stream");

const events = [];
const chunks = [];
const duplex = new Duplex({
  read() {
    this.push(null);
  },
  write(chunk, _encoding, callback) {
    chunks.push(chunk.toString());
    callback();
  },
  final(callback) {
    events.push("final");
    callback();
  }
});

duplex.on("end", () => events.push("end"));
duplex.on("finish", () => events.push("finish"));
duplex.end("value");
duplex.resume();

process.on("beforeExit", () => {
  assert.deepStrictEqual(chunks, ["value"]);
  assert.strictEqual(events.includes("final"), true);
  assert.strictEqual(events.includes("finish"), true);
  assert.strictEqual(events.includes("end"), true);
});
