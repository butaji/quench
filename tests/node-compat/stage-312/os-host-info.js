const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.release(), "string");
assert.ok(os.release().length > 0);
assert.strictEqual(os.loadavg().length, 3);
assert.ok(os.freemem() > 0);
assert.ok(os.totalmem() > 0);
assert.deepStrictEqual(os.networkInterfaces().lo, [
  {
    address: "127.0.0.1",
    netmask: "255.0.0.0",
    family: "IPv4",
    mac: "00:00:00:00:00:00",
    internal: true,
    cidr: "127.0.0.1/8",
  },
]);
