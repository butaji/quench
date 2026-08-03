const assert = require("assert");
const zlib = require("zlib");

const input = Buffer.from("callback compression callback compression");
let completed = 0;
const done = () => {
  completed++;
  if (completed === 3) console.log("zlib callbacks passed");
};

zlib.deflate(input, (error, compressed) => {
  assert.ifError(error);
  zlib.inflate(compressed, (inflateError, result) => {
    assert.ifError(inflateError);
    assert.strictEqual(result.toString(), input.toString());
    done();
  });
});

zlib.gzip(input, (error, compressed) => {
  assert.ifError(error);
  zlib.gunzip(compressed, (gunzipError, result) => {
    assert.ifError(gunzipError);
    assert.strictEqual(result.toString(), input.toString());
    done();
  });
});

assert.throws(() => zlib.deflate(input), TypeError);
zlib.inflate(Buffer.from("not compressed"), (error) => {
  assert.ok(error);
  done();
});
