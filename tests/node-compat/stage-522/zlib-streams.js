"use strict";

const assert = require("assert");
const zlib = require("zlib");

const input = Buffer.from("stream compression");
const compressed = [];
const gzip = zlib.createGzip();
gzip.on("data", (chunk) => compressed.push(chunk));
gzip.end(input);
assert.ok(compressed.length === 1);

const gunzip = zlib.createGunzip();
const result = [];
gunzip.on("data", (chunk) => result.push(chunk));
gunzip.write(compressed[0]);
assert.strictEqual(Buffer.concat(result).toString(), input.toString());

console.log("zlib streams passed");
