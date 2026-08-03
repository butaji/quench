const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("env", [], { env: { HELLO: "WORLD" } });
let output = "";
child.stdout.on("data", (chunk) => {
  output += chunk;
});
child.stdout.once("end", () => {
  assert.strictEqual(output, "HELLO=WORLD\n");
  console.log("child process stdio events passed");
});
