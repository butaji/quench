const assert = require("assert");
const moduleApi = require("module");

process.env.NODE_PATH = "/usr/test/lib/node_modules:/usr/test/lib/node:";
moduleApi._initPaths();

assert.ok(moduleApi.globalPaths.includes("/usr/test/lib/node_modules"));
assert.ok(moduleApi.globalPaths.includes("/usr/test/lib/node"));
assert.ok(!moduleApi.globalPaths.includes(""));
assert.strictEqual(moduleApi.Module.globalPaths, moduleApi.globalPaths);

console.log("module init paths pass");
