"use strict";

const assert = require("assert");
const { finished, pipeline } = require("stream/promises");
const zlib = require("zlib");

(async () => {
  const source = zlib.createGzip();
  const destination = zlib.createGunzip();
  const result = [];
  destination.on("data", (chunk) => result.push(chunk));
  const completion = finished(destination);
  source.pipe(destination);
  source.end(Buffer.from("promise pipeline"));
  await completion;
  assert.strictEqual(Buffer.concat(result).toString(), "promise pipeline");
  assert.ok(pipeline);
  console.log("stream promises passed");
})();
