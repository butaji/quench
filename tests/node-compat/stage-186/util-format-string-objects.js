const util = require("util");

if (util.format("%s", { a: [1, 2, 3] }) !== "{ a: [Array] }") {
  throw new Error("object string format mismatch");
}
if (
  util.format("%s", {
    toString() {
      return "Foo";
    },
  }) !== "Foo"
) {
  throw new Error("custom string format mismatch");
}
