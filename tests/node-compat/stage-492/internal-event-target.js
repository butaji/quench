const assert = require("assert");
const { kWeakHandler } = require("internal/event_target");

assert.strictEqual(typeof kWeakHandler, "symbol");
