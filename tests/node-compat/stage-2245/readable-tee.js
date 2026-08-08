const assert = require("assert");

const source = new ReadableStream({
  start(controller) {
    controller.enqueue("one");
    controller.enqueue("two");
    controller.close();
  }
});
const [left, right] = source.tee();
Promise.all([
  (async () => {
    const reader = left.getReader();
    assert.deepStrictEqual(await reader.read(), { value: "one", done: false });
    assert.deepStrictEqual(await reader.read(), { value: "two", done: false });
    assert.deepStrictEqual(await reader.read(), {
      value: undefined,
      done: true
    });
  })(),
  (async () => {
    const reader = right.getReader();
    assert.deepStrictEqual(await reader.read(), { value: "one", done: false });
    assert.deepStrictEqual(await reader.read(), { value: "two", done: false });
    assert.deepStrictEqual(await reader.read(), {
      value: undefined,
      done: true
    });
  })()
]).then(() => console.log("readable tee passed"));
