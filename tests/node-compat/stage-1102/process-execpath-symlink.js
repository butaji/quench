const assert = require("node:assert");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const link = path.join(os.tmpdir(), "quench-node-symlinked-node");
try {
  fs.symlinkSync(process.execPath, link);
  const result = spawnSync(link, ["child"]);
  assert.strictEqual(result.status, 0);
  assert.strictEqual(result.stdout.toString(), `${process.execPath}\n`);
} finally {
  try {
    fs.unlinkSync(link);
  } catch (_) {}
}
