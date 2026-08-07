const assert = require("node:assert");
const zlib = require("node:zlib");

for (
  const [factory, type] of [
    ["createInflate", "Inflate"],
    ["createGunzip", "Gunzip"],
    ["createUnzip", "Unzip"],
  ]
) {
  assert.ok(zlib[factory]({ windowBits: 0 }) instanceof zlib[type], factory);
}

for (const name of ["createGzip", "createDeflate"]) {
  assert.throws(() => zlib[name]({ windowBits: 0 }), {
    code: "ERR_OUT_OF_RANGE",
    name: "RangeError",
  });
}

console.log("zlib zero windowBits passed");
