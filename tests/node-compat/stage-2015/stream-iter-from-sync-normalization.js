const assert = require("assert");
const { fromSync, textSync } = require("stream/iter");

const bytes = (value) => Array.from(value);

const byteChunks = [new Uint8Array([1]), new Uint8Array([2])];
const batches = [...fromSync(byteChunks)];
assert.strictEqual(batches.length, 1);
assert.deepStrictEqual(batches[0].map(bytes), [[1], [2]]);

const nested = [
  ...fromSync(
    (function* () {
      yield ["hello", " ", "world"];
    })(),
  ),
];
assert.deepStrictEqual(
  nested[0].map((value) => new TextDecoder().decode(value)),
  ["hello", " ", "world"],
);

const protocol = {
  [Symbol.for("nodejs.stream.iter.toStreamable")]() {
    return "protocol-data";
  },
};
assert.strictEqual(textSync(fromSync(protocol)), "protocol-data");

assert.throws(() => fromSync(null), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => fromSync(12345), { code: "ERR_INVALID_ARG_TYPE" });
