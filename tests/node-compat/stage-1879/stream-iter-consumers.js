const assert = require("assert");
const {
  from,
  fromSync,
  bytes,
  bytesSync,
  text,
  textSync,
  array,
  arraySync,
  arrayBuffer,
  arrayBufferSync,
} = require("stream/iter");

(async () => {
  assert.strictEqual(textSync(fromSync("hello")), "hello");
  assert.strictEqual(await text(from("async")), "async");
  assert.deepStrictEqual(bytesSync(fromSync("ab")), new Uint8Array([97, 98]));
  assert.deepStrictEqual(await bytes(from("cd")), new Uint8Array([99, 100]));
  assert.strictEqual(new Uint8Array(arrayBufferSync(fromSync("xy"))).length, 2);
  assert.strictEqual((await arrayBuffer(from("z"))).byteLength, 1);
  assert.strictEqual(
    arraySync(fromSync([new Uint8Array([1]), new Uint8Array([2])])).length,
    2,
  );
  assert.strictEqual((await array(from([new Uint8Array([3])]))).length, 1);

  assert.throws(() => bytesSync(fromSync("hello"), { limit: 3 }), {
    name: "RangeError",
  });
  await assert.rejects(bytes(from("hello"), { limit: 3 }), {
    name: "RangeError",
  });
  await assert.rejects(bytes(from("hello"), { signal: AbortSignal.abort() }), {
    name: "AbortError",
  });
})();
