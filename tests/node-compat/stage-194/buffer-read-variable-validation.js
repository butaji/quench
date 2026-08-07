const { Buffer } = require("buffer");

for (const value of [undefined, 0, 7, 1.1]) {
  try {
    Buffer.alloc(8).readUIntLE(0, value);
    throw new Error("accepted invalid byteLength");
  } catch (error) {
    if (!["ERR_INVALID_ARG_TYPE", "ERR_OUT_OF_RANGE"].includes(error.code)) {
      throw error;
    }
  }
}
