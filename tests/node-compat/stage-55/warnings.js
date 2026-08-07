const assert = require("assert");
let received;
process.once("warning", (warning) => {
  received = warning;
});
process.emitWarning("stage warning", {
  name: "ExperimentalWarning",
  code: "STAGE",
});
assert.strictEqual(received.name, "ExperimentalWarning");
assert.strictEqual(received.code, "STAGE");
