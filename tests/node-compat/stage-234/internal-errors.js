const { codes } = require("internal/errors");

if (!(new codes.ERR_OUT_OF_RANGE() instanceof RangeError)) {
  throw new Error("internal error code missing");
}
