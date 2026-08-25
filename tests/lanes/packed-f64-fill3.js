const size = 262144;
const density = new Array(size).fill(1.25);
const u = new Array(size).fill(2.5);
const v = new Array(size).fill(3.75);

function clearFields(density, u, v) {
  for (var i = 0; i < size; i++) u[i] = v[i] = density[i] = 0;
}

for (let round = 0; round < 1000; round++) {
  density[0] = u[0] = v[0] = round + 1;
  clearFields(density, u, v);
}
if (density[0] !== 0 || u[0] !== 0 || v[0] !== 0)
  throw new Error("fill3 result");
