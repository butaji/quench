const assert = require("node:assert");
const fs = require("node:fs");

fs.utimesSync(__filename, 1, 2);
fs.lutimesSync(__filename, 1, 2);
fs.utimes(__filename, 1, 2, (error) => assert.ifError(error));
fs.lutimes(__filename, 1, 2, (error) => assert.ifError(error));

console.log("fs utimes surface passed");
