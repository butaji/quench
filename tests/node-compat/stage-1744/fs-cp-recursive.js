const fs = require("node:fs");
const path = require("node:path");
const root = path.join(process.cwd(), "tests/node/test/.tmp.0");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "cp-source.txt"), "cp-data");
fs.cpSync(path.join(root, "cp-source.txt"), path.join(root, "cp-dest.txt"));
if (fs.readFileSync(path.join(root, "cp-dest.txt"), "utf8") !== "cp-data") {
  throw new Error("cp failed");
}
console.log("fs cp file passed");
