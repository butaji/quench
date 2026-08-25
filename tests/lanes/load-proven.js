function sum(limit, value) {
  var total = 0;
  var i = 0;
  while (i < limit) {
    total += value;
    i++;
  }
  return total;
}

if (sum(250000, 1) !== 250000) throw new Error("proven load result");
if (sum(3, 0.1) !== 0.30000000000000004) throw new Error("number order");
if (sum(0, 1) !== 0) throw new Error("empty loop");
