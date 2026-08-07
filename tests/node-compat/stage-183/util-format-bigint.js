const util = require("util");

if (util.format("%d", 123n) !== "123n") {
  throw new Error("BigInt d format mismatch");
}
if (util.format("%i", 123n) !== "123n") {
  throw new Error("BigInt i format mismatch");
}
if (util.format("%f", 1n) !== "1") throw new Error("BigInt f format mismatch");
