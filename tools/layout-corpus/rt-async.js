// Layout corpus: promise jobs, async functions, generators and iterators, the run-time code
// of event-driven programs (no scored suite code). Prints one checksum.
"use strict";
class Store {
    constructor() { this.items = new Map(); this.hits = 0; }
    get size() { return this.items.size; }
    async get(key) { this.hits++; await null; return this.items.get(key); }
    async put(key, value) { await Promise.resolve(); this.items.set(key, value); return value; }
}
function* range(n) { for (let i = 0; i < n; i++) yield i; }
async function* chunks(n, size) { for (let i = 0; i < n; i += size) { await null; yield [...range(Math.min(size, n - i))].map((x) => x + i); } }
function delay(value) { return new Promise((resolve) => queueMicrotask(() => resolve(value))); }
async function request(store, id) {
    const record = { id, name: "user" + id, tags: ["a", "b", id % 3 ? "c" : "d"], at: { x: id, y: id * 2 } };
    await store.put(id, record);
    const { name, tags: [first, ...rest], at: { y } } = await store.get(id);
    const text = JSON.stringify({ name, first, rest, y });
    try {
        if (id % 17 === 0) throw new TypeError("bad " + id);
        return JSON.parse(text).y + rest.length;
    } catch (error) {
        return error.message.length;
    } finally {
        store.hits++;
    }
}
async function main() {
    const store = new Store();
    let sum = 0;
    for (let round = 0; round < 40; round++) {
        const all = await Promise.all(Array.from({ length: 50 }, (_, i) => request(store, round * 50 + i)));
        sum += all.reduce((a, b) => a + b, 0);
        sum += (await Promise.allSettled([delay(1), Promise.reject(new Error("x")), delay(2)])).filter((r) => r.status === "fulfilled").length;
        sum += await Promise.race([delay(3), delay(4)]);
        sum += await Promise.any([Promise.reject(1), delay(5)]);
        await new Promise((resolve) => setTimeout(resolve, 0));
        for await (const chunk of chunks(64, 16)) sum += chunk.length;
        const set = new Set(range(32));
        for (const [k, v] of store.items) if (set.has(k & 31)) sum += v.id & 1;
        store.items.clear();
        sum = await delay(sum).then((v) => v + 1, () => 0).catch(() => 0).finally(() => {});
    }
    console.log("rt-async", sum, store.hits);
}
main();
