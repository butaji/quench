function dot(a, b) {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

var result = dot("not an object", { x: 4, y: 5, z: 6 });
if (!Number.isNaN(result)) {
  throw new Error("property fallback assertion failed: " + result);
}
return result;
