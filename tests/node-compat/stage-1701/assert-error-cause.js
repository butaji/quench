const assert = require("node:assert");

assert.throws(
  () =>
    assert.deepStrictEqual(
      new Error("a", { cause: new Error("x") }),
      new Error("a", { cause: new Error("y") }),
    ),
  {
    message:
      "Expected values to be strictly deep-equal:\n+ actual - expected\n\n" +
      "  [Error: a] {\n" +
      "+   [cause]: [Error: x]\n" +
      "-   [cause]: [Error: y]\n" +
      "  }\n",
  },
);

console.log("assert error cause passed");
