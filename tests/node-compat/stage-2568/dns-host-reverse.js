const assert = require("node:assert");

const hostname = globalThis.__quench_dns_reverse("127.0.0.1");
assert.strictEqual(typeof hostname, "string");
assert.ok(hostname.length > 0);
console.log("DNS reverse lookup passed", hostname);
