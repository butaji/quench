const fs = require("fs");
const os = require("os");
const path = require("path");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "quench-file-url-"));
const previous = process.cwd();
process.chdir(directory);
try {
  for (let count = 0; count < 9; count++) {
    const name = `file:${
      "/".repeat(count)
    }thisDirectoryWasMadeByFailingNodeJSTestSorry/subdir`;
    try {
      fs.mkdirSync(name, { recursive: true });
      fs.writeFileSync(`${name}/file`, String(count));
      const actual = fs.readFileSync(`${name}/file`, "utf8");
      if (actual !== String(count)) throw new Error("wrong content");
    } catch (error) {
      throw new Error(`variant ${count} failed: ${error.message}`);
    }
  }
  console.log("file prefix variants passed");
} finally {
  process.chdir(previous);
}
