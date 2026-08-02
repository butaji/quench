const { Buffer } = require("buffer");

for (const value of [undefined, {}, "6", true]) {
  let rejected = false;
  try {
    Buffer.allocUnsafeSlow(value);
  } catch (error) {
    rejected = error.code === "ERR_INVALID_ARG_TYPE";
  }
  if (!rejected) throw new Error("invalid unsafe size accepted");
}
let outOfRange = false;
try {
  Buffer.allocUnsafeSlow(-1);
} catch (error) {
  outOfRange = error.code === "ERR_OUT_OF_RANGE";
}
if (!outOfRange) throw new Error("negative unsafe size accepted");
