const assert = require("node:assert");
const { AsyncLocalStorage } = require("node:async_hooks");

const storage = new AsyncLocalStorage({ defaultValue: "default" });
assert.strictEqual(storage.getStore(), "default");
assert.strictEqual(
  storage.run("run", () => storage.getStore()),
  "run",
);
assert.strictEqual(storage.getStore(), "default");

storage.enterWith("entered");
assert.strictEqual(storage.getStore(), "entered");
const scope = storage.withScope("scoped");
assert.strictEqual(storage.getStore(), "scoped");
scope.dispose();
assert.strictEqual(storage.getStore(), "entered");

const bound = AsyncLocalStorage.bind(() => storage.getStore());
assert.strictEqual(bound(), "entered");
assert.throws(() => AsyncLocalStorage.bind(null), {
  code: "ERR_INVALID_ARG_TYPE",
});

let remaining = 3;
const exitProbe = () => {
  if (remaining === 0) return;
  remaining -= 1;
  storage.run("temporary", () => storage.exit(exitProbe));
};
exitProbe();
assert.strictEqual(remaining, 0);

console.log("async local storage passed");
