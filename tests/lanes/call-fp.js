function increment(value) {
  return value + 1;
}
let result = 0;
for (let i = 0; i < 250000; i++) result = increment(result);
if (result !== 250000) throw new Error("call result");
