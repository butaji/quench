const assert = require("assert");
const { emitExperimentalWarning } = require("internal/util");

let warnings = 0;
process.on("warning", (warning) => {
  warnings += 1;
  assert.match(warning.message, /feature is an experimental feature/);
});

emitExperimentalWarning("stage-feature");
emitExperimentalWarning("stage-feature");
emitExperimentalWarning("another-stage-feature");

setImmediate(() => assert.strictEqual(warnings, 2));
