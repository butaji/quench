const { describe, it } = require("node:test");

let ran = false;
describe("compatibility", () => {
  it("runs callbacks", () => {
    ran = true;
  });
});
if (!ran) throw new Error("node:test callback did not run");
