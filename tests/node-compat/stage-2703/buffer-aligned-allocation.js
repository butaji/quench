const assert = require("assert");
const { Buffer } = require("buffer");

const pooled = Array.from({ length: 8 }, () => Buffer.allocUnsafe(64, 64));
assert.ok(pooled.some((buf, index) => index > 0 && buf.buffer === pooled[index - 1].buffer));
assert.ok(pooled.every((buf) => buf.byteOffset % 64 === 0));

const own = Buffer.allocUnsafe(64, 128);
assert.ok(pooled.every((buf) => buf.buffer !== own.buffer));

const slowA = Buffer.allocUnsafeSlow(64, 64);
const slowB = Buffer.allocUnsafeSlow(64, 64);
assert.notStrictEqual(slowA.buffer, slowB.buffer);

assert.throws(() => Buffer.allocUnsafe(10, 3), { code: "ERR_INVALID_ARG_VALUE" });
