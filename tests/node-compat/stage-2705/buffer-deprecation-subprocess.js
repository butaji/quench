const assert = require("assert");
const childProcess = require("child_process");

function run(callSite) {
  return childProcess.spawnSync(
    process.execPath,
    [
      "-p",
      `vm.runInNewContext("new Buffer(1)", { Buffer }, { filename: ${JSON.stringify(
        callSite,
      )} });`,
    ],
    { encoding: "utf8" },
  );
}

assert.strictEqual(run("/a/node_modules/x.js").stderr, "");
assert.ok(run("/a/x.js").stderr.includes("[DEP0005] DeprecationWarning"));
