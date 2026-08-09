const assert = require("assert");
const { Readable, pipeline } = require("stream");

const readable = new Readable({ read() {} });
assert.throws(() => pipeline(readable, () => {}), /ERR_MISSING_ARGS/);
assert.throws(() => pipeline(() => {}), /ERR_MISSING_ARGS/);
assert.throws(() => pipeline(), /ERR_INVALID_ARG_TYPE/);
