// spec: ECMA-262 sec-putvalue
// expect: error: TypeError
// tags: errors, type

const obj = Object.freeze({ a: 1 });
obj.a = 2;
