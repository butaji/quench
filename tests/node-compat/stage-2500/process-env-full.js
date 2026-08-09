const common = require("../../node/common");
const assert = require("assert");

if (process.argv[2] === "you-are-the-child") {
  assert.strictEqual("NODE_PROCESS_ENV_DELETED" in process.env, false);
  assert.strictEqual(process.env.NODE_PROCESS_ENV, "42");
  assert.strictEqual(process.env.hasOwnProperty, "asdf");
  assert.strictEqual(process.env[42], "forty-two");
  assert.strictEqual(Object.hasOwn(process.env, "hasOwnProperty"), true);
  return;
}

assert.strictEqual(Object.prototype.hasOwnProperty, process.env.hasOwnProperty);
assert.strictEqual(Object.hasOwn(process.env, "hasOwnProperty"), false);

process.env.hasOwnProperty = "asdf";
process.env.NODE_PROCESS_ENV = 42;
assert.strictEqual(process.env.NODE_PROCESS_ENV, "42");
process.env[42] = "forty-two";
assert.strictEqual(process.env[42], "forty-two");
process.env.NODE_PROCESS_ENV_DELETED = 42;
assert.strictEqual("NODE_PROCESS_ENV_DELETED" in process.env, true);
delete process.env.NODE_PROCESS_ENV_DELETED;
assert.strictEqual("NODE_PROCESS_ENV_DELETED" in process.env, false);

const child = require("child_process").spawn(process.argv[0], [
  __filename,
  "you-are-the-child"
]);
child.on("exit", (code) => assert.strictEqual(code, 0));

delete process.env.NON_EXISTING_VARIABLE;
assert(delete process.env.NON_EXISTING_VARIABLE);
process.env.TEST = "test";
assert.strictEqual(process.env.TEST, "test");
if (!common.isWindows) {
  assert.strictEqual(process.env.test, undefined);
  assert.strictEqual(process.env.teST, undefined);
}
assert.ok(Object.keys(process.env).length > 0);

const env = structuredClone(process.env);
assert.deepEqual(env, process.env);

process.env[""] = "";
assert.strictEqual(process.env[""], undefined);
