const fs = require("fs");
const path = require("path");
const file = path.join("/tmp", `quench-chown-${process.pid}.tmp`);
fs.writeFileSync(file, "x");
fs.chownSync(file, -1, -1);
fs.unlinkSync(file);
console.log("fs chown surface passed");
