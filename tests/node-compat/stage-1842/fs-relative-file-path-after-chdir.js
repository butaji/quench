const fs = require("fs");
const os = require("os");
const path = require("path");

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "quench-file-url-"));
const previous = process.cwd();
process.chdir(directory);
try {
  const name = "file:thisDirectoryWasMadeByFailingNodeJSTestSorry/subdir";
  fs.mkdirSync(name, { recursive: true });
  const file = `${name}/file`;
  fs.writeFileSync(file, "ok");
  if (fs.readFileSync(file, "utf8") !== "ok") {
    throw new Error("relative file path read failed");
  }
  console.log("relative file path after chdir passed");
} finally {
  process.chdir(previous);
}
