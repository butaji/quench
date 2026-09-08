function dot(a, b) {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

dot({ x: 1, y: 2, z: 3 }, { x: 2, y: 3, z: 4 });
var result = dot(
  { x: 1.5, y: -2, z: 4 },
  { x: 2, y: 3, z: 5 }
);
if (result !== 17) {
  throw new Error("vector field dot mismatch");
}
return result;
