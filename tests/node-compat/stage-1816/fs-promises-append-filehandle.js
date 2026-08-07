const assert = require("assert");
const fs = require("fs");

const path = "stage-1816-append.txt";
fs.writeFileSync(path, "a");

fs.promises
  .open(path, "a+")
  .then((handle) =>
    fs.promises.appendFile(handle, "b").then(() => handle.close())
  )
  .then(() => {
    assert.strictEqual(fs.readFileSync(path, "utf8"), "ab");
    fs.unlinkSync(path);
    console.log("fs promises append FileHandle passed");
  });
