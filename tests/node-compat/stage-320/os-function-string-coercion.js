const assert = require("assert");
const os = require("os");

for (
  const name of [
    "hostname",
    "homedir",
    "release",
    "type",
    "endianness",
    "tmpdir",
    "arch",
    "platform",
    "version",
    "machine",
  ]
) {
  assert.strictEqual(`${os[name]}`, os[name]());
}
