const assert = require("assert");
assert.doesNotThrow(() => {
  console.time("stage-50");
  console.timeLog("stage-50", "working");
  console.timeEnd("stage-50");
});
