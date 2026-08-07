const { Buffer } = require("buffer");

const source = Buffer.from([1, 2, 3, 4]);
const target = Buffer.alloc(4);
if (source.copy(target, 1, 1, 3) !== 2) throw new Error("copy count failed");
if (target[1] !== 2 || target[2] !== 3) throw new Error("copy bytes failed");

const overlap = Buffer.from([1, 2, 3, 4]);
if (overlap.copy(overlap, 1, 0, 3) !== 3) {
  throw new Error("overlap count failed");
}
if (overlap.toString("hex") !== "01010203") {
  throw new Error("overlap copy failed");
}
