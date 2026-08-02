const assert = require("assert");
const { format } = require("util");

class Bar {
  constructor() {
    this.abc = true;
  }
}

assert.strictEqual(format("%s", new Bar()), "Bar { abc: true }");
