const assert = require("assert");
const path = require("path");
const { spawn } = require("child_process");

const fixture = path.join(__dirname, "argv0-child.fixture");
const child = spawn(process.execPath, [fixture, "child"]);
let output = "";

child.stdout.on("data", (chunk) => {
  assert(Buffer.isBuffer(chunk));
  output += chunk;
});
child.on("close", () => {
  assert.strictEqual(output, process.execPath);
});
