// Full-collection cost with many live closures over many environments. Run with --expose-gc.
// Prints the median of ROUNDS forced collections, in milliseconds.
const CLOSURES = 200_000;
const ROUNDS = 15;
function make(i) {
  const a = { i }, b = [i, i + 1], c = 'c' + i;
  const sibling = () => b.length + c.length;
  sibling();
  return () => a.i;
}
const kept = [];
for (let i = 0; i < CLOSURES; i++) kept.push(make(i));
gc();
const times = [];
for (let round = 0; round < ROUNDS; round++) {
  const started = process.hrtime.bigint();
  gc();
  times.push(Number(process.hrtime.bigint() - started) / 1e6);
}
times.sort((x, y) => x - y);
let sum = 0;
for (const f of kept) sum += f();
console.log(JSON.stringify({ closures: CLOSURES, medianGcMs: times[ROUNDS >> 1].toFixed(2), heapUsedMiB: (process.memoryUsage().heapUsed / 1048576).toFixed(1), check: sum }));
