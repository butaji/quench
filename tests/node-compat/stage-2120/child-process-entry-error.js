const assert = require("assert");
const { execFileSync } = require("child_process");

for (const entry of ["iDoNotExist", "iDoNotExist.js", "iDoNotExist.mjs"]) {
  assert.throws(
    () => execFileSync(process.execPath, [entry], { stdio: "pipe" }),
    (error) =>
      error.code === "MODULE_NOT_FOUND" &&
      error.toString().includes("Cannot find module") &&
      error.toString().includes(entry)
  );
}

console.log("child process entry error pass");
