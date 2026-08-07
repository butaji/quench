const buffer = require("buffer");
const { Buffer } = buffer;
const util = require("util");

buffer.INSPECT_MAX_BYTES = 2;
const slow = Buffer.allocUnsafeSlow(4).fill("1234");
let rendered;
try {
  rendered = util.inspect(slow);
} catch (error) {
  throw new Error(`inspect threw: ${error && error.message}`);
}
if (rendered !== "<Buffer 31 32 ... 2 more bytes>") {
  throw new Error(`inspect mismatch: ${rendered}`);
}
const decorated = Buffer.alloc(2);
decorated.inspect = undefined;
decorated.prop = new Uint8Array(0);
if (
  util.inspect(decorated) !==
    "<Buffer 00 00, inspect: undefined, prop: Uint8Array(0) []>"
) {
  throw new Error("named property inspect mismatch");
}
