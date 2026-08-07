const { Buffer } = require("buffer");
const first = Buffer.from("ab");
const second = new Uint8Array([0x63, 0x64]);
const combined = Buffer.concat([first, second]);
if (combined.toString() !== "abcd") {
  throw new Error("concat did not copy views");
}
if (Buffer.concat([first], 6).length !== 6) {
  throw new Error("concat length was ignored");
}
