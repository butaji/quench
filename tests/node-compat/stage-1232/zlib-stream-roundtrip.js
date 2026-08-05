const assert = require("assert");
const zlib = require("zlib");

const compressed = [];
const restored = [];
zlib
  .createGzip()
  .on("data", (chunk) => compressed.push(chunk))
  .on("end", () => {
    zlib
      .createGunzip()
      .on("data", (chunk) => restored.push(chunk))
      .on("end", () => {
        assert.strictEqual(Buffer.concat(restored).toString(), "hello zlib");
      })
      .end(Buffer.concat(compressed));
  })
  .end("hello zlib");
