const assert = require("node:assert");
const { URLSearchParams } = require("node:url");
for (
  const name of [
    "append",
    "delete",
    "get",
    "getAll",
    "has",
    "set",
    "sort",
    "toString",
  ]
) {
  assert.strictEqual(
    Object.getOwnPropertyDescriptor(URLSearchParams.prototype, name).enumerable,
    true,
    name,
  );
}
for (
  const name of [
    "append",
    "delete",
    "get",
    "getAll",
    "has",
    "set",
    "sort",
    "toString",
  ]
) {
  assert.strictEqual(
    Object.hasOwn(
      Object.getOwnPropertyDescriptor(URLSearchParams.prototype, name).value,
      "prototype",
    ),
    false,
    name,
  );
}
for (const name of ["entries", "forEach", "keys", "values"]) {
  assert.strictEqual(
    Object.hasOwn(
      Object.getOwnPropertyDescriptor(URLSearchParams.prototype, name).value,
      "prototype",
    ),
    false,
    name,
  );
}
for (
  const name of [Symbol.iterator, Symbol.for("nodejs.util.inspect.custom")]
) {
  assert.strictEqual(
    Object.hasOwn(
      Object.getOwnPropertyDescriptor(URLSearchParams.prototype, name).value,
      "prototype",
    ),
    false,
    String(name),
  );
}
for (
  const [name, methodName] of [
    ["entries", "entries"],
    ["forEach", "forEach"],
    ["keys", "keys"],
    ["values", "values"],
    [Symbol.iterator, "entries"],
    [Symbol.for("nodejs.util.inspect.custom"), "[nodejs.util.inspect.custom]"],
  ]
) {
  const value = Object.getOwnPropertyDescriptor(
    URLSearchParams.prototype,
    name,
  ).value;
  assert.strictEqual(value.name, methodName, String(name));
  assert.strictEqual(Object.hasOwn(value, "prototype"), false, String(name));
}
console.log("URLSearchParams method descriptors passed");
