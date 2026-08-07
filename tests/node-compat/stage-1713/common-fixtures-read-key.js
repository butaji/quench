const assert = require("assert");
const fixtures = require("../../node/test/common/fixtures");

const key = fixtures.readKey("rsa_public.pem");
assert.ok(Buffer.isBuffer(key));
assert.ok(key.toString("utf8").includes("BEGIN PUBLIC KEY"));
assert.strictEqual(
  fixtures.readKey("rsa_public.pem", "utf8").includes("BEGIN PUBLIC KEY"),
  true,
);
assert.ok(
  fixtures
    .path("keys", "rsa_public.pem")
    .endsWith("/fixtures/keys/rsa_public.pem"),
);
