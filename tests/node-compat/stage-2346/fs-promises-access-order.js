const assert = require("assert");
const fs = require("fs");

const events = [];
fs.access(__filename, () => events.push("callback"));
fs.promises.access(__filename).then(() => events.push("promise"));
setImmediate(() => {
  assert.deepStrictEqual(events, ["callback", "promise"]);
  console.log("fs promise access order passed");
});
