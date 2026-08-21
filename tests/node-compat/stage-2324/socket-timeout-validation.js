const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
for (
  const value of [
    "100",
    true,
    false,
    undefined,
    null,
    "",
    {},
    () => {},
    [],
  ]
) {
  assert.throws(() => socket.setTimeout(value, () => {}), {
    code: "ERR_INVALID_ARG_TYPE",
  });
}
for (const value of [-1, Infinity, -Infinity, NaN]) {
  assert.throws(() => socket.setTimeout(value), { code: "ERR_OUT_OF_RANGE" });
}
assert.ok(net.Server());
console.log("socket timeout validation passed");
