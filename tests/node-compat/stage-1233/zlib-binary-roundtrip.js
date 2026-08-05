const assert = require("assert");
const zlib = require("zlib");

const source = Buffer.from([0, 1, 2, 127, 128, 200, 254, 255]);
const compressed = [];
const restored = [];
zlib
  .createGzip()
  .on("data", (chunk) => compressed.push(chunk))
  .on("end", () => {
    zlib
      .createGunzip()
      .on("data", (chunk) => restored.push(chunk))
      .on("end", () => assert.deepStrictEqual(Buffer.concat(restored), source))
      .end(Buffer.concat(compressed));
  })
  .end(source);
