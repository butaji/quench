// Nullish coalescing operator tests
// ECMA-262 sec-logical-or-operator

// Test 1: null ?? 'default' returns 'default'
let a = null;
let b = a ?? 42;
b;
