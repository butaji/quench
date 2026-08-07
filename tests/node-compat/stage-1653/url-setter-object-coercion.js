const assert = require("node:assert");
const url = new URL("https://github.com/");
const value = {
  toString() {
    throw new Error("toString");
  },
};
assert.throws(() => {
  url.protocol = value;
}, /^Error: toString$/);
console.log("URL setter object coercion passed");
