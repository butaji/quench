const assert = require("assert");
const { Readable, Transform, pipeline } = require("stream");

pipeline(
  new Readable({
    read() {
      this.push("data");
      this.push(null);
    },
  }),
  new Transform({
    readableObjectMode: true,
    transform(chunk, encoding, callback) {
      this.push({ chunk: String(chunk) });
      callback();
    },
  }),
  (readable) => readable.map(async (value) => value.chunk),
  (error) => assert.ifError(error),
);
