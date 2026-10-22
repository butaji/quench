var values = [];
for (var initialized = 0; initialized < 1000; initialized++) values.push(0);
for (var round = 0; round < 1000; round++) {
  for (var index = 0; index < 1000; index++) values[index] = round + index;
}
if (values[0] !== 999 || values[999] !== 1998) throw new Error("dense overwrite mismatch");
