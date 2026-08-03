const assert = require("assert");
const net = require("net");

assert.strictEqual(net.isIPv4("127.0.0.1"), true);
assert.strictEqual(net.isIPv4("0.0.0.0"), true);
assert.strictEqual(net.isIPv4("255.255.255.255"), true);
assert.strictEqual(net.isIPv4("256.0.0.0"), false);
assert.strictEqual(net.isIPv4("1.2.3"), false);
assert.strictEqual(net.isIPv4("01.2.3.4"), false);
assert.strictEqual(net.isIPv4("::1"), false);
assert.strictEqual(net.isIPv4("example.com"), false);
assert.strictEqual(net.isIPv4(""), false);

assert.strictEqual(net.isIP("127.0.0.1"), 4);
assert.strictEqual(net.isIP("::1"), 6);
assert.strictEqual(net.isIP("example.com"), 0);
assert.strictEqual(net.isIP(""), 0);
assert.strictEqual(net.isIP("0000:0000:0000:0000:0000:0000:0000:0000"), 6);
assert.strictEqual(net.isIP("::"), 6);
assert.strictEqual(net.isIP("::ffff:192.168.1.1"), 6);

console.log("net isip passed");
