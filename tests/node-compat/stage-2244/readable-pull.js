const assert = require("assert");

const stream = new ReadableStream({
  pull(controller) {
    controller.enqueue("value");
    controller.close();
  },
});
const reader = stream.getReader();
Promise.all([
  reader
    .read()
    .then((item) =>
      assert.deepStrictEqual(item, { value: "value", done: false })
    ),
  reader
    .read()
    .then((item) =>
      assert.deepStrictEqual(item, { value: undefined, done: true })
    ),
]).then(() => console.log("readable pull passed"));
