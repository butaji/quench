const assert = require("assert");
const fs = require("fs");

for (const value of [false, 1, {}, [], null, undefined]) {
  assert.throws(() => fs.linkSync(value, "/tmp/unused"), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
  assert.throws(() => fs.linkSync("/tmp/unused", value), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
}

const source = `/tmp/quench-node-stage-111-source-${process.pid}`;
const callbackLink = `${source}-callback`;
const promiseLink = `${source}-promise`;
fs.writeFileSync(source, "hello");
fs.link(source, callbackLink, (error) => {
  assert.ifError(error);
  assert.strictEqual(fs.readFileSync(callbackLink, "utf8"), "hello");
  fs.promises.link(source, promiseLink).then(() => {
    assert.strictEqual(fs.readFileSync(promiseLink, "utf8"), "hello");
    fs.unlinkSync(source);
    fs.unlinkSync(callbackLink);
    fs.unlinkSync(promiseLink);
  });
});

console.log("link passed");
