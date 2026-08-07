const assert = require("node:assert");
const prefix =
  "Expected values to be strictly deep-equal:\n+ actual - expected\n\n";
assert.throws(
  () =>
    assert.deepStrictEqual(
      new Error("a"),
      new Error("a", { cause: { prop: "value" } }),
    ),
  {
    message:
      `${prefix}+ [Error: a]\n- [Error: a] {\n-   [cause]: {\n-     prop: 'value'\n-   }\n- }\n`,
  },
);
assert.throws(
  () =>
    assert.deepStrictEqual(
      new Error("a"),
      new Error("a", { cause: undefined }),
    ),
  {
    message:
      `${prefix}+ [Error: a]\n- [Error: a] {\n-   [cause]: undefined\n- }\n`,
  },
);
assert.throws(
  () =>
    assert.deepStrictEqual(
      new Error("a", { cause: undefined }),
      new Error("a"),
    ),
  {
    message:
      `${prefix}+ [Error: a] {\n+   [cause]: undefined\n+ }\n- [Error: a]\n`,
  },
);
console.log("assert object cause passed");
