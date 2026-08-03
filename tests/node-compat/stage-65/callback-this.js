const assert = require("assert");
const common = require("../common");
let receiver;
common.mustCall(function () {
  receiver = this;
})();
assert.strictEqual(receiver, undefined);
