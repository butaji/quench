const assert = require("assert");
const { VirtualFileHandle } = require("internal/vfs/file_handle");

const handle = new VirtualFileHandle("/x.txt", "r", 0o600);
assert.strictEqual(handle.path, "/x.txt");
assert.strictEqual(handle.flags, "r");
assert.strictEqual(handle.mode, 0o600);
assert.throws(() => handle.readSync(), { code: "ERR_METHOD_NOT_IMPLEMENTED" });
assert.rejects(handle.read(), { code: "ERR_METHOD_NOT_IMPLEMENTED" });
handle.closeSync();
assert.strictEqual(handle.closed, true);
assert.throws(() => handle.readSync(), { code: "EBADF" });
