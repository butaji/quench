const assert = require("node:assert");
const fs = require("node:fs");

const fd = fs.openSync("write-file-descriptor.txt", "w+");
fs.writeFile(fd, "data", (error) => assert.ifError(error));
console.log("write file descriptor passed");
