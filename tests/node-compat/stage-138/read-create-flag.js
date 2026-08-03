const fs = require("fs");
const path = `/tmp/quench-node-stage-138-${process.pid}`;
const value = fs.readFileSync(path, { flag: "a+", encoding: "utf8" });
if (value !== "" || !fs.existsSync(path)) throw new Error("a+ read mismatch");
