const util = require("util");

if (util.format("%d", Symbol()) !== "NaN") {
  throw new Error("symbol decimal mismatch");
}
if (util.format("%i", Symbol()) !== "NaN") {
  throw new Error("symbol integer mismatch");
}
if (util.format("%f", Symbol()) !== "NaN") {
  throw new Error("symbol float mismatch");
}
if (util.format("%f", Symbol("foo")) !== "NaN") {
  throw new Error("named symbol float mismatch");
}
