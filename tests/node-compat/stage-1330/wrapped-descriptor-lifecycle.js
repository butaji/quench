const assert = require("node:assert");
const fs = require("node:fs");

fs._open = fs.open;
fs._close = fs.close;
let openCount = 0;
fs.open = (...args) => {
  openCount++;
  return fs._open(...args);
};
fs.close = (...args) => {
  openCount--;
  return fs._close(...args);
};

fs.open("wrapped-descriptor.txt", "w", (openError, descriptor) => {
  assert.ifError(openError);
  fs.fchmod(descriptor, 0o751, (chmodError) => {
    assert.ifError(chmodError);
    assert.strictEqual(fs.fstatSync(descriptor).mode & 0o777, 0o751);
    fs.close(descriptor, (closeError) => {
      assert.ifError(closeError);
      fs.unlinkSync("wrapped-descriptor.txt");
    });
  });
});

setTimeout(() => assert.strictEqual(openCount, 0), 10);
