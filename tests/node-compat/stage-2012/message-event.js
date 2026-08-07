const assert = require("assert");

const event = new MessageEvent("message", { data: 42, origin: "origin" });
assert.strictEqual(event.type, "message");
assert.strictEqual(event.data, 42);
assert.strictEqual(event.origin, "origin");
assert.deepStrictEqual(event.ports, []);
