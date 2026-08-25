const elements = [3, 5, 7, 11];
let total = 0;
for (let i = 0; i < 25000000; i++) total += elements[i & 3];
if (total !== 162500000) throw new Error("array index result");
