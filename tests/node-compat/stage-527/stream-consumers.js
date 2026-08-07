"use strict";

const assert = require("assert");
const { json, text, buffer } = require("stream/consumers");
const { ReadableStream } = require("stream/web");

(async () => {
  const makeStream = () =>
    new ReadableStream({
      start(controller) {
        controller.enqueue(Buffer.from('{"ok":'));
        controller.enqueue(Buffer.from("true}"));
        controller.close();
      },
    });
  assert.strictEqual(await text(makeStream()), '{"ok":true}');
  assert.deepStrictEqual(await json(makeStream()), { ok: true });
  assert.strictEqual((await buffer(makeStream())).length, 11);
  console.log("stream consumers passed");
})();
