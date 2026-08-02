const assert = require("assert");
const { format } = require("util");

class Foo {}
const value = Object.setPrototypeOf(new Foo(), null);
assert.strictEqual(format("%s", value), "[Foo: null prototype] {}");
