const assert = require("node:assert");
const child = require("node:child_process");

function run(main, callSite, warning) {
  const result = child.spawnSync(
    process.execPath,
    [
      "-p",
      `process.mainModule = { filename: ${JSON.stringify(main)} };
       vm.runInNewContext('new Buffer(10)', { Buffer }, { filename: ${
        JSON.stringify(callSite)
      } });`,
    ],
    { encoding: "utf8" },
  );
  assert.strictEqual(typeof result.stdout, "string");
  assert.strictEqual(typeof result.stderr, "string");
  assert.strictEqual(
    result.stderr.includes("[DEP0005] DeprecationWarning"),
    warning,
  );
}

run("/a/node_modules/b.js", "/a/node_modules/x.js", false);
run("/a.js", "/b.js", true);
run("/node_modules/a.js.js", "/b.js", true);
console.log("child process encoding and Buffer warning passed");
