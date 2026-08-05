const assert = require("node:assert");
const fs = require("node:fs");

const stream = new fs.createReadStream(__filename, { end: 1 });
assert(stream);
console.log("read stream constructor passed");
