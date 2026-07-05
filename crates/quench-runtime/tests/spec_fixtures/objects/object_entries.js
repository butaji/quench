// spec: ECMA-262 sec-object.entries
// expect: value: [["a", 1], ["b", 2]]
// tags: objects, entries

const obj = { a: 1, b: 2 };
Object.entries(obj);
