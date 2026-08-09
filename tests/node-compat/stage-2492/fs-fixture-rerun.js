const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(process.cwd(), "stage-2492-rerun");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(root);

const file = path.join(root, "file.txt");
const link = path.join(root, "link");
for (let iteration = 0; iteration < 2; iteration++) {
  if (fs.existsSync(file)) fs.chmodSync(file, 0o644);
  fs.writeFileSync(file, String(iteration));
  fs.chmodSync(file, 0o444);
  fs.rmSync(link, { force: true });
  fs.symlinkSync("file.txt", link);
  assert.strictEqual(fs.readlinkSync(link), "file.txt");
}

fs.chmodSync(file, 0o644);
fs.rmSync(root, { recursive: true, force: true });
