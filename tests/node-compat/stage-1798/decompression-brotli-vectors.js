"use strict";
const assert = require("assert");
const zlib = require("zlib");
const { DecompressionStream } = require("stream/web");

const valid = zlib.brotliCompressSync(Buffer.from("a"));
const empty = new Uint8Array(1);
const cases = [
  [new Uint8Array([...valid, ...empty])],
  [valid, empty],
  [valid, valid],
  [new Uint8Array([...valid, ...valid])],
];
Promise.all(cases.map((chunks) =>
  assert.rejects(
    Array.fromAsync(
      new Blob(chunks).stream().pipeThrough(new DecompressionStream("brotli")),
    ),
    { name: "TypeError" },
  )
)).then(() => console.log("decompression brotli vectors passed"));
