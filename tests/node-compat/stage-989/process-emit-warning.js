const { test } = require("node:test");
test("process emitWarning emits a warning event", async () => {
  const warning = await new Promise((resolve) => {
    process.once("warning", resolve);
    process.emitWarning("compatibility warning", "CustomWarning");
  });
  if (!(warning instanceof Error)) throw new Error("warning was not an Error");
  if (warning.name !== "CustomWarning") {
    throw new Error("warning name was lost");
  }
});
