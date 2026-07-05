// spec: ECMA-262 sec-for-in-and-for-of-statements-runtime-semantics
// expect: value: "ab"
// tags: statements, for-in

const obj = { a: 1, b: 2 };
let keys = "";
for (const k in obj) {
  keys += k;
}
keys;
