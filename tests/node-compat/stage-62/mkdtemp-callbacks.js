const assert = require("assert");
const fs = require("fs");
const common = require("../common");
const folder = fs.mkdtempSync("/tmp/quench-node-stage-62-");
function done(error, value) {
  assert.ifError(error);
  assert.strictEqual(fs.existsSync(value), true);
}
fs.mkdtemp(`${folder}/a.`, common.mustCall(done));
fs.mkdtemp(`${folder}/b.`, {}, common.mustCall(done));
fs.mkdtemp(`${folder}/c.`, common.mustCall(done));
