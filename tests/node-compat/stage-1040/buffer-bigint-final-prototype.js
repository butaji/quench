const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8);
for (const endianness of ["LE", "BE"]) {
  const value = -123456789n;
  assert.strictEqual(buffer[`writeBigInt64${endianness}`](value, 0), 8);
  assert.strictEqual(buffer[`readBigInt64${endianness}`](0), value);
  assert.strictEqual(buffer[`writeBigUInt64${endianness}`](123n, 0), 8);
  assert.strictEqual(buffer[`readBigUInt64${endianness}`](0), 123n);
}
