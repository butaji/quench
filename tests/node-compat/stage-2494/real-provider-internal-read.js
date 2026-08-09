const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");
const { getVirtualFd } = require("internal/vfs/fd");

const root = path.join(process.cwd(), "stage-2494-real-provider");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "file.txt"), "internal read");

const provider = vfs.create(new vfs.RealFSProvider(root));
const fd = provider.openSync("/file.txt", "r");
const originalFstatSync = fs.fstatSync;
fs.fstatSync = () => {
  throw new Error("public fstatSync must not be used");
};
try {
  assert.strictEqual(
    getVirtualFd(fd).entry.readFileSync("utf8"),
    "internal read"
  );
} finally {
  fs.fstatSync = originalFstatSync;
  provider.closeSync(fd);
}
