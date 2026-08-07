const assert = require("assert");
const { format } = require("util");

class Foobar extends Array {
  constructor(length) {
    super(length);
    this.aaa = true;
  }
}

assert.strictEqual(
  format("%s", new Foobar(5)),
  "Foobar(5) [ <5 empty items>, aaa: true ]",
);
