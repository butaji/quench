const assert = require("assert");

assert.strictEqual(typeof navigator.userAgent, "string");
assert.ok(navigator.userAgent.startsWith("Node.js/"));
assert.strictEqual(navigator.language, "en-US");
assert.deepStrictEqual(navigator.languages, ["en-US"]);
assert.strictEqual(typeof navigator.hardwareConcurrency, "number");
assert.ok(navigator.hardwareConcurrency > 0);
assert.strictEqual(typeof navigator.platform, "string");
assert.ok(Object.isFrozen(navigator));

console.log("navigator global passed");
