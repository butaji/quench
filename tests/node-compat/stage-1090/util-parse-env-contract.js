const assert = require("node:assert");
const fs = require("node:fs");
const util = require("node:util");

const fixture = fs.readFileSync(
  "tests/node/test/fixtures/dotenv/valid.env",
  "utf8",
);
const values = util.parseEnv(fixture);

assert.strictEqual(values.BASIC, "basic");
assert.strictEqual(
  values.INLINE_COMMENTS_SPACE,
  "inline comments start with a",
);
assert.strictEqual(
  values.MULTI_DOUBLE_QUOTED,
  "THIS\nIS\nA\nMULTILINE\nSTRING",
);
assert.deepStrictEqual(util.parseEnv("FOO=bar\nFOO=baz\n"), {
  __proto__: null,
  FOO: "baz",
});
assert.throws(() => util.parseEnv(null), { code: "ERR_INVALID_ARG_TYPE" });
