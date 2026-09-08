"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = -8;
  for (let outer = 3; outer < 9; outer++)
    for (let middle = 1; middle < 6; middle++)
      for (let inner = 5; inner < 12; inner++) total = ((inner ^ (outer ^ middle)) & 15) + total;
  return total;
}
function verify(result) {
  assert(result === 1651, "reordered nested xor reduction");
  return result;
}
return { run, verify };
