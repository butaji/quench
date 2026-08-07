const assert = require("assert");
const { merge, text, from } = require("stream/iter");

(async () => {
  assert.strictEqual(await text(merge(from("ab"), from("cd"))), "abcd");
  assert.strictEqual(await text(merge("x", "y")), "xy");
  assert.strictEqual(await text(merge()), "");
  await assert.rejects(
    text(merge(from("x"), { signal: AbortSignal.abort() })),
    { name: "AbortError" },
  );
})();
