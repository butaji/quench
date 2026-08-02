const assert = require("assert");
const { codes } = require("internal/errors");
const util = require("util");

const error = new codes.ERR_IPC_CHANNEL_CLOSED();
assert.strictEqual(error.code, "ERR_IPC_CHANNEL_CLOSED");
assert.strictEqual(util.types.isNativeError(error), true);
