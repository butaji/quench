const assert = require("assert");
const crypto = require("crypto");
const id = crypto.randomUUID();
assert.match(
  id,
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
);
assert.notStrictEqual(id, crypto.randomUUID());
