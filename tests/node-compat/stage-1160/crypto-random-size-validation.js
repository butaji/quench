const crypto = require("crypto");

for (const value of [undefined, null, false, true, {}, []]) {
  try {
    crypto.randomBytes(value);
    throw new Error("invalid random size was accepted");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}

const output = crypto.randomBytes(8);
if (output.length !== 8) throw new Error("random byte length mismatch");
