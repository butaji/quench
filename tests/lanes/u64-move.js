let x = 1;
let y = 0;
for (let i = 0; i < 25000000; i++) {
  y = x;
  x = y;
}
if (x !== 1 || y !== 1) throw new Error("move result");
