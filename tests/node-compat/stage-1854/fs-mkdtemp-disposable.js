const assert = require("assert");
const fs = require("fs");

const result = fs.mkdtempDisposableSync(
  `/tmp/quench-disposable-${process.pid}-`,
);
assert(fs.existsSync(result.path));
assert.strictEqual(typeof result.remove, "function");
assert.strictEqual(typeof result[Symbol.dispose], "function");
result.remove();
assert(!fs.existsSync(result.path));
result[Symbol.dispose]();
console.log("fs mkdtemp disposable passed");
