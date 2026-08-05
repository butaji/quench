const assert = require("node:assert");
const { URL } = require("node:url");

const descriptor = Object.getOwnPropertyDescriptor(URL.prototype, "href");
assert.strictEqual(typeof descriptor.set, "function");
const url = new URL("https://example.com");
url.href = "https://example.org/updated";
assert.strictEqual(url.href, "https://example.org/updated");
console.log("URL href setter passed");
