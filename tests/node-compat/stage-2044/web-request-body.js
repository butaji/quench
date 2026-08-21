const assert = require("assert");

(async () => {
  const request = new Request("http://example.test/", {
    body: JSON.stringify({ value: 7 }),
  });
  assert.deepStrictEqual(await request.json(), { value: 7 });
  assert.strictEqual(await new Request("x", { body: "ok" }).text(), "ok");
  console.log("web request body passed");
})();
