"use strict";
const assert = require("assert");
const { DecompressionStream } = require("stream/web");

const valid = new Uint8Array([
  31,
  139,
  8,
  0,
  0,
  0,
  0,
  0,
  0,
  19,
  75,
  4,
  0,
  67,
  190,
  183,
  232,
  1,
  0,
  0,
  0,
]);
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
      new Blob(chunks).stream().pipeThrough(new DecompressionStream("gzip")),
    ),
    { name: "TypeError" },
  )
)).then(() => console.log("decompression gzip vectors passed"));
