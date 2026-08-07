const assert = require("node:assert");

for (const method of ["toString", "toJSON"]) {
  assert.throws(
    () => URL.prototype[method].call({}),
    /Receiver must be an instance/,
  );
}
for (const property of ["href", "search"]) {
  assert.throws(
    () => Reflect.get(URL.prototype, property, {}),
    /Receiver must be an instance/,
  );
}
for (
  const property of [
    "protocol",
    "username",
    "password",
    "host",
    "hostname",
    "port",
    "pathname",
    "hash",
    "origin",
    "searchParams",
  ]
) {
  assert.throws(
    () => Reflect.get(URL.prototype, property, {}),
    /Cannot read private member/,
  );
}
console.log("URL invalid receiver checks passed");
