const assert = require("assert");
const {
  from,
  fromSync,
  pull,
  pullSync,
  tap,
  tapSync,
  text,
  textSync,
} = require("stream/iter");

(async () => {
  const asyncSeen = [];
  const asyncObserver = tap(async (chunks) => {
    if (chunks !== null) asyncSeen.push(chunks.length);
  });
  const asyncResult = await text(pull(from("hello"), asyncObserver));
  assert.strictEqual(asyncResult, "hello");
  assert.deepStrictEqual(asyncSeen, [1]);

  const syncSeen = [];
  const syncObserver = tapSync((chunks) => {
    if (chunks === null) syncSeen.push("flush");
    else syncSeen.push(chunks.length);
  });
  assert.strictEqual(
    textSync(pullSync(fromSync("world"), syncObserver)),
    "world",
  );
  assert.deepStrictEqual(syncSeen, [1, "flush"]);

  await assert.rejects(
    (async () => {
      for await (
        const _ of pull(
          from("x"),
          tap(() => {
            throw new Error("tap error");
          }),
        )
      ) {}
    })(),
    { message: "tap error" },
  );
})();
