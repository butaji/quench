"use strict";
const { createHash, createHmac } = require("node:crypto");
const algorithms = ["sha256", "sha512", "sha1", "md5"];
const key = new Uint8Array([0, 255, 1, 128, 42]);
const hashes = Array.from({ length: 48 }, (_, index) =>
  createHash(algorithms[index % algorithms.length]).update(`prefix ${index} é水🦄`));
const hmacs = Array.from({ length: 24 }, (_, index) =>
  createHmac(algorithms[index % algorithms.length], key).update(`prefix ${index} é水🦄`));
console.log(JSON.stringify([
  hashes.map((hash, index) => hash.update(` suffix ${index}`).digest("hex")),
  hmacs.map((hash, index) => hash.update(` suffix ${index}`).digest("hex")),
  algorithms.map(algorithm => {
    const hash = createHash(algorithm).update("copy prefix");
    const copy = hash.copy();
    return [hash.update(" original").digest("hex"), copy.update(" copied").digest("hex")];
  }),
  createHash("sha256").update("live across").update(" collection").digest("hex"),
]));
