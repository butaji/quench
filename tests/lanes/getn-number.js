const object = { value: 1 };
let sum = 0;
for (let i = 0; i < 250000; i++) sum += object.value;
if (sum !== 250000) throw new Error("property result");
