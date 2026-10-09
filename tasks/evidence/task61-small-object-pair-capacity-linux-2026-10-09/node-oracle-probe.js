function pair(n) { return { first: n, second: n + 1 }; }
const items = Array.from({ length: 5000 }, (_, i) => pair(i));
const sample = items[4095];
Object.defineProperty(sample, 'sum', { enumerable: true, get() { return this.first + this.second; } });
const before = [sample.first, sample.second, sample.sum, Object.keys(sample)];
delete sample.first;
sample.third = 9;
const after = [sample.first, sample.second, sample.sum, Object.keys(sample), Object.getOwnPropertyDescriptor(sample, 'second')];
const duplicate = { x: 1, y: 2, x: 3 };
console.log(JSON.stringify({ before, after, duplicate, count: items.length, last: [items.at(-1).first, items.at(-1).second] }));
