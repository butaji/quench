const assert = require("assert");
const { compose, Duplex, PassThrough } = require("stream");

class BufferedDuplex extends Duplex {
  _read() {}

  _write(_chunk, _encoding, callback) {
    callback();
  }

  _destroy(error, callback) {
    callback(error);
  }
}

const first = new PassThrough({ objectMode: true });
const last = new BufferedDuplex({ objectMode: true });
first.on("error", () => {});
last.on("error", () => {});

const composed = compose(first, last).on("error", () => {});
const failure = new Error("compose failed");
composed.destroy(failure);

assert.strictEqual(composed.destroyed, true);
assert.strictEqual(first.destroyed, true);
assert.strictEqual(last.destroyed, true);
