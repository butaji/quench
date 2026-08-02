const { Buffer } = require("buffer");

if (Buffer.from("ab\ud800cd").toString("hex") !== "6162efbfbd6364") {
  throw new Error("lone surrogate UTF-8 replacement failed");
}
