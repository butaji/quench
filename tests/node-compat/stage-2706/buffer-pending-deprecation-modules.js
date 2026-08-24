const assert = require("assert");
const childProcess = require("child_process");

for (const extension of ["cjs.js", "esm.mjs"]) {
  const fixture = `tests/node/test/fixtures/warning_node_modules/new-buffer-${extension}`;
  const quiet = childProcess.spawnSync(process.execPath, [fixture], {
    encoding: "utf8",
  });
  assert.strictEqual(quiet.status, 0);
  assert.strictEqual(quiet.stderr, "");

  const pending = childProcess.spawnSync(
    process.execPath,
    ["--pending-deprecation", fixture],
    { encoding: "utf8" },
  );
  assert.strictEqual(pending.status, 0);
  assert.ok(pending.stderr.includes("[DEP0005] DeprecationWarning"));
}
