const assert = require("assert");
process.on("beforeExit", () => process.exit(0));
process.on("exit", (code) => assert.strictEqual(code, 0));
