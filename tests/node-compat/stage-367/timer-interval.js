const assert = require("assert");
let count = 0;
const handle = setInterval(() => {
  count++;
  if (count === 3) {
    clearInterval(handle);
    queueMicrotask(() => assert.strictEqual(count, 3));
  }
}, 1);
