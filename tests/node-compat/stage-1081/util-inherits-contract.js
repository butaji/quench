const assert = require("assert");
const { inherits } = require("util");

function Parent() {}
function Child() {}

inherits(Child, Parent);
assert.strictEqual(Child.super_, Parent);
assert.strictEqual(new Child().constructor, Child);
