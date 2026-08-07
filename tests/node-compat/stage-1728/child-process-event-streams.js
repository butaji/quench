const assert = require("node:assert");
const child = require("node:child_process").spawn("echo", ["ok"]);
assert.ok(child.stdout && child.stderr && child.stdin);
let spawned = false;
child.on("spawn", () => {
  spawned = true;
});
child.stdout.on(
  "data",
  (value) => assert.strictEqual(value.toString(), "ok\n"),
);
child.on("close", () => {
  assert.strictEqual(spawned, true);
  console.log("child process event streams passed");
});
