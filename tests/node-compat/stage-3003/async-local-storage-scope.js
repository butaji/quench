"use strict";
const assert = require("assert");
const { AsyncLocalStorage } = require("async_hooks");

const storage = new AsyncLocalStorage({ defaultValue: "default" });
assert.strictEqual(storage.getStore(), "default");
const outer = storage.withScope("outer");
assert.strictEqual(storage.getStore(), "outer");
const inner = storage.withScope("inner");
assert.strictEqual(storage.getStore(), "inner");
inner.dispose();
assert.strictEqual(storage.getStore(), "outer");
outer.dispose();
outer.dispose();
assert.strictEqual(storage.getStore(), "default");
