function sum(limit) {
  let value = 1;
  let total = 0;
  for (let i = 0; i < limit; i++) total += value;
  return total;
}

if (sum(250000) !== 250000) throw new Error("proven load result");
