const util = require("util");

if (util.format("%f", Symbol("foo")) !== "NaN") {
  throw new Error("symbol numeric formatting failed");
}
