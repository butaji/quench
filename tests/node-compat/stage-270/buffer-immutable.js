const { Buffer } = require("buffer");

const backing = new ArrayBuffer(4);
new Uint8Array(backing).set([1, 2, 3, 4]);
const immutable = Buffer.from(backing.transferToImmutable());
const source = Buffer.from([9, 9, 9, 9]);
if (source.copy(immutable) !== 0 || immutable[0] !== 1) {
  throw new Error("immutable destination was modified");
}
const target = Buffer.alloc(4);
if (immutable.copy(target) !== 4 || target[0] !== 1) {
  throw new Error("immutable source could not be read");
}
