const assert = require("assert");

assert.throws(
  () => require("../../node/test/fixtures/module-loading-error.node"),
  (error) =>
    error instanceof Error &&
    error.code === "ERR_DLOPEN_FAILED" &&
    error.message.includes("file too short")
);

console.log("module native addon error pass");
