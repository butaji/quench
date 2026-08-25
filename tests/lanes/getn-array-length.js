const elements = [];
let total = 0;
for (let i = 0; i < 25000000; i++) total += elements.length;
if (total !== 0) throw new Error("array length result");
