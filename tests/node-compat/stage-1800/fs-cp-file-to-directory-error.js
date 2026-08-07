const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(path.join(process.cwd(), "cp-error-"));
const source = path.join(root, "source.txt");
const destination = path.join(root, "destination");
fs.writeFileSync(source, "source");
fs.mkdirSync(destination);
assert.throws(() => fs.cpSync(source, destination), {
  code: "ERR_FS_CP_NON_DIR_TO_DIR",
});
console.log("fs cp file-to-directory error passed");
