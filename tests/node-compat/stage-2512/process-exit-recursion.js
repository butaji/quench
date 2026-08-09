const assert = require("assert");
let exits = 0;

process.on("exit", (code) => {
  assert.strictEqual(exits++, 0);
  assert.strictEqual(code, 0);
  process.exit();
});
