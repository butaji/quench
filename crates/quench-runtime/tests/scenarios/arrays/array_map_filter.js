// Array map and filter
// ECMA-262 sec-array.prototype.map, sec-array.prototype.filter

let arr = [1, 2, 3, 4, 5];

// Test map
let mapped = arr.map(function(x) { return x * 2; });
mapped.join(",");

// Test filter
let filtered = arr.filter(function(x) { return x > 2; });
filtered.join(",");
