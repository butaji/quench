import assert from "assert";
import stream, { PassThrough, Readable } from "stream";

assert.strictEqual(Readable, stream.Readable);
assert.strictEqual(PassThrough, stream.PassThrough);
for (const method of ["some", "every", "find", "reduce"]) {
  assert.strictEqual(typeof Readable.prototype[method], "function");
}

assert.strictEqual(
  await Readable.from([1, 2, 3]).some((value) => value > 2),
  true,
);
assert.strictEqual(
  await Readable.from([1, 2, 3]).every((value) => value < 4),
  true,
);
assert.strictEqual(
  await Readable.from([1, 2, 3]).find((value) => value > 1),
  2,
);
assert.strictEqual(
  await Readable.from([1, 2, 3]).reduce((total, value) => total + value, 0),
  6,
);
