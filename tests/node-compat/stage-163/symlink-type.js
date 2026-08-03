const fs = require("fs");
try {
  fs.symlinkSync("", "", "invalid");
  throw new Error("accepted invalid symlink type");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_VALUE") throw error;
}
