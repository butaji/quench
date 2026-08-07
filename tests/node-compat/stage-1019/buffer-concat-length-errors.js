const { Buffer } = require("buffer");
for (
  const [length, phrase] of [
    [3.5, "must be an integer"],
    [-1, "must be >= 0"],
  ]
) {
  try {
    Buffer.concat([Buffer.from("a")], length);
    throw new Error("invalid concat length was accepted");
  } catch (error) {
    if (error.code !== "ERR_OUT_OF_RANGE" || !error.message.includes(phrase)) {
      throw new Error("concat length error was not descriptive");
    }
  }
}
