const assert = require("assert");
const { Duplex, PassThrough } = require("stream");

const response = new PassThrough();
assert.strictEqual(typeof response.once, "function");
class Probe extends Duplex {
  constructor() {
    super({ autoDestroy: false });
    assert.strictEqual(typeof response.once, "function");
    assert.strictEqual(typeof this.push, "function");
  }
  _read() {}
  _write(chunk, encoding, callback) {
    callback();
  }
}
new Probe();
