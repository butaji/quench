const assert = require("assert");

assert.deepStrictEqual(
  [...Buffer.from("ababc", "ucs2")],
  [0x61, 0, 0x62, 0, 0x61, 0, 0x62, 0, 0x63, 0],
);
console.log("buffer ucs2 string passed");
