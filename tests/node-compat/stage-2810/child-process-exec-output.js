const assert = require("assert");
const { exec } = require("child_process");

let callbackOutput = "";
const child = exec(
  "/usr/bin/env",
  { env: { HELLO: "WORLD" } },
  (error, stdout) => {
    assert.strictEqual(error, null);
    callbackOutput = stdout;
    assert.notStrictEqual(stdout, "");
  },
);
child.stdout.setEncoding("utf8");
child.stdout.on("data", (chunk) => assert.strictEqual(typeof chunk, "string"));
process.on("exit", () => assert.notStrictEqual(callbackOutput, ""));
