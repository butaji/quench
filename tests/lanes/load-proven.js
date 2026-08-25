function sum(limit, value) {
  let total = 0;
  for (let i = 0; i < limit; i++) total += value;
  return total;
}

if (sum(250000, 1) !== 250000) throw new Error("proven load result");
if (sum(3, 0.1) !== 0.30000000000000004) throw new Error("number order");
if (sum(0, 1) !== 0) throw new Error("empty loop");
