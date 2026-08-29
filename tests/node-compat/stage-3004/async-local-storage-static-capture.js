"use strict";

const assert = require("assert");
const { AsyncLocalStorage } = require("async_hooks");

const storage = new AsyncLocalStorage();
storage.enterWith("captured");

const bound = AsyncLocalStorage.bind(() => storage.getStore());
storage.enterWith("changed");
assert.strictEqual(bound(), "captured");

const snapshot = AsyncLocalStorage.snapshot();
storage.enterWith("changed-again");
assert.strictEqual(snapshot(() => storage.getStore()), "changed");
