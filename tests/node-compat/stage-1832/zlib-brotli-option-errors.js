const zlib = require("zlib");

for (const name of ["flush", "finishFlush"]) {
  let threw = false;
  try {
    zlib.brotliCompressSync("", { [name]: zlib.constants.Z_FINISH });
  } catch (error) {
    threw = error.code === "ERR_OUT_OF_RANGE";
  }
  if (!threw) throw new Error(`${name} validation missing`);
}

console.log("zlib Brotli option errors passed");
