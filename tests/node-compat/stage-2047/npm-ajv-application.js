const assert = require("assert");
const Ajv = require("ajv");

const ajv = new Ajv({ allErrors: true });
const validate = ajv.compile({
  type: "object",
  required: ["name", "count"],
  properties: {
    name: { type: "string" },
    count: { type: "integer", minimum: 1 }
  },
  additionalProperties: false
});

assert.strictEqual(validate({ name: "quench", count: 1 }), true);
assert.strictEqual(validate({ name: "quench", count: 0 }), false);
assert.ok(validate.errors?.some((error) => error.keyword === "minimum"));
console.log("npm ajv application passed");
