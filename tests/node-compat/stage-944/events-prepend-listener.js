const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
const calls = [];
const first = () => calls.push("first");
const prepended = () => calls.push("prepended");

emitter.on("ready", first);
assert.strictEqual(emitter.prependListener("ready", prepended), emitter);
emitter.emit("ready");
assert.deepStrictEqual(calls, ["prepended", "first"]);
