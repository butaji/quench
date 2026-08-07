const { parse } = require("querystring");
const input = Array.from({ length: 10000 }, (_, index) => `${index}=x`).join(
  "&",
);
if (
  Object.keys(parse(input, undefined, undefined, { maxKeys: Infinity }))
    .length !== 10000
) {
  throw new Error("numeric Infinity was limited");
}
if (
  Object.keys(parse(input, undefined, undefined, { maxKeys: NaN })).length !==
    10000
) {
  throw new Error("numeric NaN was limited");
}
if (
  Object.keys(parse(input, undefined, undefined, { maxKeys: "Infinity" }))
    .length !== 1000
) {
  throw new Error("string Infinity ignored the default limit");
}
