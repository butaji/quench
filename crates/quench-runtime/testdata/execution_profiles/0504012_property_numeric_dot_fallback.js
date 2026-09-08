function dot(a, b) {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

function run() {
  return dot("not an object", { x: 4, y: 5, z: 6 });
}
function verify(result) {
if (!Number.isNaN(result)) {
  throw new Error("property fallback assertion failed: " + result);
}
  return result;
}
return { run: run, verify: verify };
