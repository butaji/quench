const assert = require("assert");
assert.throws(() => require("package.json"), { code: "MODULE_NOT_FOUND" });
