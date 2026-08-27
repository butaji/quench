const assert = require("assert");
const { EventEmitter } = require("events");

const emitter = new EventEmitter();
let calls = 0;
emitter.once("value", (value) => {
  calls += value;
});
emitter.emit("value", 2);
emitter.emit("value", 3);
assert.strictEqual(calls, 2);
assert.strictEqual(emitter.listenerCount("value"), 0);
console.log("events once contract pass");
