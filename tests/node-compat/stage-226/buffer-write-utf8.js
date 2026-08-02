const { Buffer } = require("buffer");

const buffer = Buffer.alloc(2);
if (buffer.write("\0あ") !== 1) throw new Error("UTF-8 partial write failed");
if (buffer[0] !== 0) throw new Error("UTF-8 write byte mismatch");
