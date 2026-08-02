const { Buffer } = require("buffer");

const buffer = Buffer.alloc(3, 7).fill(null);
if (buffer.toString("hex") !== "000000") throw new Error("null fill failed");
