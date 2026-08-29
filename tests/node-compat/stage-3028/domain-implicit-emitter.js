"use strict";

const assert = require("assert");
const EventEmitter = require("events");
const domain = require("domain").create();

let emitter;
domain.on("error", (error) => {
  assert.strictEqual(error.domain, domain);
  assert.strictEqual(error.domainEmitter, emitter);
});
domain.run(() => {
  emitter = new EventEmitter();
});
setImmediate(() => emitter.emit("error", new Error("boom")));
