const assert = require("assert");
const { runInNewContext } = require("vm");
const value = runInNewContext("new String('test')");
assert.deepStrictEqual(Buffer.from(value), Buffer.from("test"));
