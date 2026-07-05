// spec: ECMA-262 sec-math.random
// expect: value: true
// tags: math, random

const r = Math.random();
r >= 0 && r < 1;
