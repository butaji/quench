const assert = require("assert");

assert.strictEqual(Buffer.from([0xff, 0xfe, 0x61]).toString(), "��a");
assert.strictEqual(Buffer.from([0xf0, 0x80, 0x80, 0x80]).toString(), "����");
