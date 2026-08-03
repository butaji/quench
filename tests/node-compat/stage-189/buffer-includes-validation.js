const { Buffer } = require("buffer");

for (const value of [() => {}, {}, []]) {
  try {
    Buffer.from("abc").includes(value);
    throw new Error("accepted invalid includes value");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
