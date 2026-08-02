const assert = require("assert");
const os = require("os");

const info = os.userInfo();
assert.strictEqual(typeof info.username, "string");
assert.strictEqual(typeof info.uid, "number");
assert.strictEqual(typeof info.gid, "number");
assert.strictEqual(typeof info.shell, "string");
const buffered = os.userInfo({ encoding: "buffer" });
assert.strictEqual(buffered.username.toString(), info.username);
assert.strictEqual(buffered.shell.toString(), info.shell);
