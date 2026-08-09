const assert = require("assert");
const { toWritable } = require("stream/iter");

const batches = [];
const writable = toWritable({
  write() {
    return Promise.resolve();
  },
  writev(chunks) {
    batches.push(chunks);
    return Promise.resolve();
  },
  end() {
    return Promise.resolve();
  }
});
writable.cork();
writable.write("a");
writable.write("b");
writable.uncork();
(async () => {
  await Promise.resolve();
  assert.strictEqual(batches.length, 1);
  assert.strictEqual(batches[0].length, 2);
})();
