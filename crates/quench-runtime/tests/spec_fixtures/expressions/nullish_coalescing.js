// spec: ecma-262 sec-binary-logical-operators-runtime-semantics
// expect: value: 42
// tags: nullish, coalescing, expressions

var a = null;
a ?? 42;
