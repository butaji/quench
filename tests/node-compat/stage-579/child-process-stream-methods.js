const assert = require("assert");
const child = require("child_process").spawn("node", []);

for (const stream of [child.stdin, child.stdout, child.stderr]) {
  assert.strictEqual(typeof stream.on, "function");
  assert.strictEqual(typeof stream.once, "function");
  assert.strictEqual(typeof stream.setEncoding, "function");
  assert.strictEqual(stream.setEncoding("utf8"), stream);
}

console.log("child process stream methods passed");
