const assert = require("node:assert");
const { EventEmitter } = require("events");
const events = new EventEmitter();
let value = 0;
const listener = (amount) => {
  value += amount;
};
events.on("add", listener);
events.emit("add", 2);
assert.strictEqual(value, 2);
assert.strictEqual(events.listenerCount("add"), 1);
events.once("add", listener);
events.emit("add", 3);
assert.strictEqual(value, 8);
events.off("add", listener);
assert.strictEqual(events.listenerCount("add"), 0);
