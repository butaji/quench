const assert = require("assert");
const { promisify } = require("util");

let warnings = 0;
const handler = () => { warnings++; };
process.on("warning", handler);
function callback() {}
callback.constructor = (async () => {}).constructor;
promisify(callback);
process.off("warning", handler);
assert.strictEqual(warnings, 0);
