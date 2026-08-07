"use strict";
const assert = require("assert");
const { DecompressionStream } = require("stream/web");
const valid = new Uint8Array([120, 156, 75, 4, 0, 0, 98, 0, 98]);
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
      new Blob(chunks).stream().pipeThrough(new DecompressionStream("deflate")),
    ),
    { name: "TypeError" },
  )
)).then(() => console.log("decompression deflate vectors passed"));
