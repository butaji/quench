const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("coap://[FEDC:BA98:7654:3210:FEDC:BA98:7654:3210]");
assert.strictEqual(parsed.hostname, "fedc:ba98:7654:3210:fedc:ba98:7654:3210");
assert.strictEqual(parsed.host, "[fedc:ba98:7654:3210:fedc:ba98:7654:3210]");
assert.strictEqual(
  url.parse("coap://[1080:0:0:0:8:800:200C:417A]:61616/").port,
  "61616",
);
console.log("legacy IPv6 URL hosts passed");
