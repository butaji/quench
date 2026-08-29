"use strict";

const assert = require("assert");
const domain = require("domain");
const EventEmitter = require("events");

const parent = domain.create();
const child = domain.create();
let childCalls = 0;
let parentCalls = 0;

parent.on("error", (error) => {
  parentCalls += 1;
  assert.strictEqual(error.message, "child handler");
});
child.on("error", (error) => {
  childCalls += 1;
  assert.strictEqual(error.message, "original");
  throw new Error("child handler");
});

parent.run(() => {
  child.run(() => {
    const emitter = new EventEmitter();
    child.add(emitter);
    emitter.emit("error", new Error("original"));
  });
});

assert.strictEqual(childCalls, 1);
assert.strictEqual(parentCalls, 1);
