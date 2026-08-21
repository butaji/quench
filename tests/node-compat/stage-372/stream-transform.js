const assert = require("assert");
const { Transform } = require("stream");
const values = [];
const transform = new Transform({
  transform(chunk, _encoding, callback) {
    this.push(String(chunk).toUpperCase());
    callback();
  },
});
transform.on("data", (value) => values.push(value));
transform.write("hello", () => {
  assert.deepStrictEqual(values, [Buffer.from("HELLO")]);
});
