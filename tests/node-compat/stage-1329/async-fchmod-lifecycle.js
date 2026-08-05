const assert = require("node:assert");
const fs = require("node:fs");

fs.open("async-fchmod.txt", "w", (openError, descriptor) => {
  assert.ifError(openError);
  fs.fchmod(descriptor, 0o751, (chmodError) => {
    assert.ifError(chmodError);
    assert.strictEqual(fs.fstatSync(descriptor).mode & 0o777, 0o751);
    fs.close(descriptor, (closeError) => {
      assert.ifError(closeError);
      fs.unlinkSync("async-fchmod.txt");
      console.log("async fchmod lifecycle passed");
    });
  });
});
