"use strict";
(function () {
  let identity = function identity(value) { return value; };
  function capturedLeafLoop(count, value) {
    let sum = 0;
    for (let i = 0; i < count; i++) sum += identity(value);
    return sum;
  }
  let checksum = 0;
  for (let warm = 0; warm < 40; warm++) checksum += capturedLeafLoop(2000, 2);
  identity = function replacement(value) { return value + 7; };
  const after = capturedLeafLoop(2000, 2);
  if (checksum !== 160000 || after !== 18000) throw new Error("captured-leaf parity");
  console.log(JSON.stringify([checksum, after]));
})();
