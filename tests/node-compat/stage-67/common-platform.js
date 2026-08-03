const assert = require("assert");
const common = require("../common");
assert.strictEqual(typeof common.isLinux, "boolean");
assert.strictEqual(typeof common.isMacOS, "boolean");
assert.strictEqual(common.mustNotMutateObjectDeep({ answer: 42 }).answer, 42);
assert.strictEqual(require("worker_threads").isMainThread, true);
