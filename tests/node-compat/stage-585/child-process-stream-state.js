const assert = require("assert");
const child = require("child_process").spawn("node", []);

assert.strictEqual(child.stdin.readable, false);
assert.strictEqual(child.stdin.writable, true);
for (const stream of [child.stdout, child.stderr]) {
  assert.strictEqual(stream.readable, true);
  assert.strictEqual(stream.writable, true);
  assert.strictEqual(stream.destroyed, false);
}

console.log("child process stream state passed");
