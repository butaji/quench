"use strict";
const assert = require("assert");
const { AsyncLocalStorage } = require("async_hooks");

const storage = new AsyncLocalStorage();
storage.enterWith("outer");
assert.strictEqual(storage.getStore(), "outer");
assert.strictEqual(storage.exit(() => storage.getStore()), undefined);
assert.strictEqual(storage.getStore(), "outer");
assert.strictEqual(storage.exit(() => 42), 42);
