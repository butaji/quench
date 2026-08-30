"use strict";

const assert = require("assert");
const { EventTarget, CustomEvent } = require("internal/event_target");

assert.strictEqual(typeof CustomEvent, "function");
const event = new CustomEvent("ready", { detail: 42 });
assert.strictEqual(event.type, "ready");
assert.strictEqual(event.detail, 42);
assert.ok(new EventTarget());
