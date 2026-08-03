const assert = require("assert");
const zlib = require("zlib");

const input = Buffer.from("hello world hello world hello world");

// deflateSync / inflateSync
const deflated = zlib.deflateSync(input);
assert.ok(Buffer.isBuffer(deflated));
assert.strictEqual(zlib.inflateSync(deflated).toString(), input.toString());

// deflateRawSync / inflateRawSync
const raw = zlib.deflateRawSync(input);
assert.ok(Buffer.isBuffer(raw));
assert.strictEqual(zlib.inflateRawSync(raw).toString(), input.toString());

// gzipSync / gunzipSync
const gz = zlib.gzipSync(input);
assert.ok(Buffer.isBuffer(gz));
assert.strictEqual(zlib.gunzipSync(gz).toString(), input.toString());

// isZlib
assert.strictEqual(zlib.isZlib(), true);

// string input
const s = "compress me compress me compress me";
const cd = zlib.deflateSync(s);
assert.strictEqual(zlib.inflateSync(cd).toString(), s);

// options
const def = zlib.deflateSync(input, { level: 9 });
assert.ok(Buffer.isBuffer(def));

console.log("zlib sync passed");
