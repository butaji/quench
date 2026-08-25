function increment(value) {
  return value + 1;
}
let result = 0;
for (let i = 0; i < 25000000; i++) result = increment(result);
if (result !== 25000000) throw new Error("call result");
