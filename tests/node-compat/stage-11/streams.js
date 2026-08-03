const assert = require("assert");
const { Readable, Writable } = require("node:stream");
const chunks = [];
const output = new Writable({});
output.on("data", (chunk) => chunks.push(chunk));
Readable.from(["a", "b"]).pipe(output);
queueMicrotask(() => assert.deepStrictEqual(chunks, ["a", "b"]));
