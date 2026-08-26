const assert = require("assert");

const actual = [{ a: 1 }, 2, 3, 4, { c: [1, 2, 3] }];
const expected = [{ a: 1 }, 2, 3, 4, { c: [3, 4, 5] }];

assert.throws(
  () => assert.deepStrictEqual(actual, expected),
  {
    code: "ERR_ASSERTION",
    name: "AssertionError",
    message:
      "Expected values to be strictly deep-equal:\n" +
      "+ actual - expected\n" +
      "... Skipped lines\n" +
      "\n" +
      "  [\n" +
      "    {\n" +
      "      a: 1\n" +
      "    },\n" +
      "    2,\n" +
      "...\n" +
      "      c: [\n" +
      "+       1,\n" +
      "+       2,\n" +
      "        3,\n" +
      "-       4,\n" +
      "-       5\n" +
      "      ]\n" +
      "    }\n" +
      "  ]\n",
  },
);
