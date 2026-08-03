"use strict";

const assert = require("assert");
const { compressGzip, decompressGzip } = require("zlib/iter");

(async () => {
  const input = [Buffer.from("iterable "), Buffer.from("compression")];
  const compressed = [];
  for await (const chunk of compressGzip()(input)) compressed.push(chunk);
  const output = [];
  for await (const chunk of decompressGzip()(compressed)) output.push(chunk);
  assert.strictEqual(Buffer.concat(output).toString(), "iterable compression");
  console.log("zlib iter passed");
})();
