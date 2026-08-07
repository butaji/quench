const assert = require("assert");
const { from, fromSync, text, textSync } = require("stream/iter");

(async () => {
  const latin1 = new Uint8Array([0xe9, 0xe8, 0xea]);
  assert.strictEqual(
    await text(from(latin1), { encoding: "iso-8859-1" }),
    "éèê",
  );
  assert.strictEqual(
    textSync(fromSync(latin1), { encoding: "iso-8859-1" }),
    "éèê",
  );

  const split = (async function* () {
    yield [new Uint8Array([0xe2, 0x82])];
    yield [new Uint8Array([0xac])];
  })();
  assert.strictEqual(await text(split), "€");
  assert.strictEqual(
    await text(from(new Uint8Array([0xef, 0xbb, 0xbf, 0x68, 0x69]))),
    "hi",
  );
  await assert.rejects(text(from(new Uint8Array([0xff, 0xfe]))), {
    name: "TypeError",
  });
  await assert.rejects(text(from("hello"), { encoding: 1 }), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  await assert.rejects(
    text(from("hello"), { encoding: "not-a-real-encoding" }),
    { name: "RangeError" },
  );
})();
