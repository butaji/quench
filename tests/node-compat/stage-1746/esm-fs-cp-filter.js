const fs = require("node:fs");
const path = require("node:path");
const root = path.join(process.cwd(), "tests/node/test/.tmp.0/esm-cp");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(path.join(root, "src"), { recursive: true });
fs.writeFileSync(path.join(root, "src", "keep.js"), "keep");
fs.writeFileSync(path.join(root, "src", "skip.txt"), "skip");
fs.cp(path.join(root, "src"), path.join(root, "dest"), {
  recursive: true,
  filter: (source) =>
    source.endsWith("keep.js") || fs.statSync(source).isDirectory(),
}, (error) => {
  if (error) throw error;
  if (!fs.existsSync(path.join(root, "dest", "keep.js"))) {
    throw new Error("filtered copy missing");
  }
  if (fs.existsSync(path.join(root, "dest", "skip.txt"))) {
    throw new Error("filter failed");
  }
  console.log("esm fs cp filter passed");
});
