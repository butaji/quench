import assert from "node:assert";

const request = new Request("http://example.test/", {
  method: "post",
  headers: { "x-test": "yes" },
});
assert.strictEqual(request.method, "POST");
assert.strictEqual(request.headers.get("X-Test"), "yes");

const response = new Response("hello", { status: 201 });
assert.strictEqual(response.status, 201);
assert.strictEqual(response.ok, true);
assert.strictEqual(await response.text(), "hello");
console.log("web request response passed");
