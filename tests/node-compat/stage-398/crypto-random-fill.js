const crypto = require("crypto");

const buffer = Buffer.alloc(8, 0);
crypto.randomFill(buffer, 2, 3, (error, result) => {
  if (error || result !== buffer) throw error || new Error("wrong buffer");
  if (buffer[0] !== 0 || buffer[1] !== 0 || buffer[5] !== 0) {
    throw new Error("randomFill wrote outside the requested range");
  }
  if (buffer[2] === 0 && buffer[3] === 0 && buffer[4] === 0) {
    throw new Error("randomFill did not write the requested range");
  }
  console.log("crypto random fill passed");
});
