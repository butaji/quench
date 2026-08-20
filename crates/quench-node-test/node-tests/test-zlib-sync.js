// zlib — real gzip/gunzip and deflateRaw/inflateRaw round-trips (flate2).
'use strict';
const assert = require('assert');
const zlib = require('node:zlib');

const data = 'hello quench zlib '.repeat(4);

const gz = zlib.gzipSync(Buffer.from(data, 'utf8'));
assert.ok(gz.length < data.length, 'gzip compresses');
assert.strictEqual(zlib.gunzipSync(gz).toString('utf8'), data, 'gzip round-trip');

const raw = zlib.deflateRawSync(Buffer.from(data, 'utf8'));
assert.ok(raw.length < data.length, 'deflateRaw compresses');
assert.strictEqual(zlib.inflateRawSync(raw).toString('utf8'), data, 'deflateRaw round-trip');

const z = zlib.deflateSync(Buffer.from(data, 'utf8'));
assert.strictEqual(zlib.inflateSync(z).toString('utf8'), data, 'zlib deflate round-trip');

console.log('zlib: ok');