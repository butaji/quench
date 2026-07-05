// spec: ECMA-262 sec-object.assign
// expect: value: {a: 1, b: 2, c: 3}
// tags: objects, assign

const target = { a: 1 };
const source = { b: 2, c: 3 };
Object.assign(target, source);
target;
