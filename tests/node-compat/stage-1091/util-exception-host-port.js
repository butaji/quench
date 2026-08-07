const assert = require("node:assert");
const os = require("node:os");
const util = require("node:util");

const error = util._exceptionWithHostPort(
  -os.constants.errno.ENOENT,
  "connect",
  "127.0.0.1",
  8080,
  "0.0.0.0:12345",
);
assert.strictEqual(error.code, "ENOENT");
assert.strictEqual(error.errno, -2);
assert.strictEqual(error.address, "127.0.0.1");
assert.strictEqual(error.port, 8080);
assert.match(error.message, /127\.0\.0\.1:8080 - Local/);

const noPort = util._exceptionWithHostPort(-2, "connect", "127.0.0.1", 0);
assert.strictEqual(noPort.port, undefined);
assert.strictEqual(noPort.message, "connect ENOENT 127.0.0.1");
