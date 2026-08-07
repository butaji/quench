const assert = require("assert");
const EventEmitter = require("events");
const emitter = Object.create(EventEmitter.prototype);
let called = false;
emitter.on("data", () => (called = true));
emitter.emit("data", "value");
assert.strictEqual(called, true);
console.log("event emitter lazy initialization passed");
