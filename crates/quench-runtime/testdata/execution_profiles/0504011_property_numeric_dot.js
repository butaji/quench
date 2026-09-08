function dot(a, b) {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

function run() {
  return dot(
  { x: 1, y: 2, z: 3 },
  { x: 4, y: 5, z: 6 }
);
}
function verify(result) {
if (result !== 32) {
  throw new Error("dot product assertion failed: " + result);
}
  return result;
}
return { run: run, verify: verify };
