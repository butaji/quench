const assert = require("assert");
const { Duplex } = require("stream");

class SourceDuplex extends Duplex {
  constructor() {
    super({ autoDestroy: false });
  }
  _read() {
    this.push("value");
    this.push(null);
  }
  _write(chunk, encoding, callback) {
    callback();
  }
}

const stream = new SourceDuplex();
let value = "";
stream.on("data", (chunk) => (value += chunk.toString()));
stream.resume();
setImmediate(() => assert.strictEqual(value, "value"));
