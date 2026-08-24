"use strict";

const assert = require("assert");
const EventEmitter = require("events");
const domain = require("domain");

const emitter = new EventEmitter();
const d = domain.create();
d.add(emitter);
d.on("error", (error) => {
  assert.strictEqual(error.message, "boom");
  assert.strictEqual(error.domain, d);
  assert.strictEqual(error.domainEmitter, emitter);
  assert.strictEqual(error.domainThrown, false);
});
emitter.emit("error", new Error("boom"));
