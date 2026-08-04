const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const calls = [];
const regular = () => calls.push("regular");
const once = () => calls.push("once");

emitter.on("ready", regular);
assert.strictEqual(emitter.prependOnceListener("ready", once), emitter);
emitter.emit("ready");
emitter.emit("ready");
assert.deepStrictEqual(calls, ["once", "regular", "regular"]);
