const assert = require("assert");
const Hono = require("hono");
(async () => {
  const app = new Hono();
  app.get("/health", (c) => c.json({ ok: true, framework: "hono" }));
  const response = await app.request("http://127.0.0.1/health");
  assert.strictEqual(response.status, 200);
  assert.deepStrictEqual(await response.json(), { ok: true, framework: "hono" });
  console.log("hono fetch passed");
})();
