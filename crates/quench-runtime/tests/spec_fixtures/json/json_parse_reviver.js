// spec: ECMA-262 sec-json.parse
// expect: value: {a: 2}
// tags: json, parse, reviver

const json = '{"a": 1, "b": 2}';
const obj = JSON.parse(json, (key, value) => {
  if (key === "b") return undefined;
  return value;
});
obj;
