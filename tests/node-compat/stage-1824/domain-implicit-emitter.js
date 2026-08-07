const assert = require("assert");
const domain = require("domain");
const EventEmitter = require("events");

const d = new domain.Domain();
let emitter;
d.on("error", (error) => {
  assert.strictEqual(error.message, "foobar");
  assert.strictEqual(error.domain, d);
  assert.strictEqual(error.domainEmitter, emitter);
  assert.strictEqual(error.domainThrown, false);
  console.log("domain implicit emitter passed");
});
d.run(() => {
  emitter = new EventEmitter();
});
setTimeout(() => emitter.emit("error", new Error("foobar")), 1);
