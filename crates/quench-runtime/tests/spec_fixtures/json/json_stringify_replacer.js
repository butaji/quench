// spec: ECMA-262 sec-json.stringify
// expect: value: '{"a":1}'
// tags: json, stringify, replacer

const obj = { a: 1, b: 2 };
const json = JSON.stringify(obj, (key, value) => {
  if (key === "b") return undefined;
  return value;
});
json;
