const assert = require("assert");
const fs = require("fs");

const stream = fs.createReadStream(__filename);
let callbacks = 0;
stream.close(() => callbacks++);
stream.close(() => callbacks++);
setTimeout(() => {
  assert.strictEqual(callbacks, 2);
  console.log("fs read stream close idempotent passed");
}, 0);
