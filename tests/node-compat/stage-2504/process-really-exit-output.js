const assert = require("assert");
const { spawnSync } = require("child_process");

if (process.argv[2] === "subprocess") {
  process.reallyExit = () => console.info("really exited");
  process.exit();
}

const result = spawnSync(process.execPath, [__filename, "subprocess"]);
assert.strictEqual(result.status, 0);
assert.strictEqual(result.stdout.toString("utf8").trim(), "really exited");
