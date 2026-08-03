const assert = require("assert");
const childProcess = require("child_process");

process.env.QUENCH_CHILD_ENV = "inherited";
const child = childProcess.spawn("env", []);
let output = "";
child.stdout.on("data", (chunk) => {
  output += chunk;
});
child.stdout.once("end", () => {
  assert.strictEqual(output.includes("QUENCH_CHILD_ENV=inherited\n"), true);
  console.log("child process env inheritance passed");
});
