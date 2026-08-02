const { runInNewContext } = require("vm");

if (runInNewContext("value + 1", { value: 4 }) !== 5) {
  throw new Error("vm context evaluation failed");
}
