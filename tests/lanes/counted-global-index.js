var table = [];
var key, value;
key = 48;
for (value = 0; value < 4096; ++value) table[key++] = value;
if (
  key !== 4144 ||
  value !== 4096 ||
  table[48] !== 0 ||
  table[49] !== 1 ||
  table[4143] !== 4095
)
  throw new Error("counted loop exposed a stale global index");
