const size = 262144;
const x = new Array(size).fill(1.25);
const source = new Array(size).fill(0.5);
const dt = 0.01;

function addFields(x, source, dt) {
  for (var i = 0; i < size; i++) x[i] += dt * source[i];
}

for (let round = 0; round < 1000; round++) addFields(x, source, dt);
if (!(x[0] > 1.7)) throw new Error("add fields result");
