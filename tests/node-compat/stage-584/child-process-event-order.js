const assert = require("assert");
const child = require("child_process").spawn("echo", ["ok"]);
let spawned = false;
child.on("spawn", () => {
  spawned = true;
});
for (const stream of [child.stdout, child.stderr]) {
  stream.on("end", () => assert.strictEqual(spawned, true));
  stream.on("close", () => assert.strictEqual(spawned, true));
}
child.on("exit", () => assert.strictEqual(spawned, true));
child.on("close", () => {
  assert.strictEqual(spawned, true);
  console.log("child process event order passed");
});
