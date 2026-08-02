const assert = require("assert");
const { Readable, Writable } = require("stream");
const received = [];
const destination = new Writable({ highWaterMark: 1 });
destination.write = (chunk, callback) => {
  received.push(chunk);
  if (received.length === 3) assert.deepStrictEqual(received, [1, 2, 3]);
  queueMicrotask(() => {
    destination.emit("drain");
    if (callback) callback();
  });
  return false;
};
Readable.from([1, 2, 3]).pipe(destination);
