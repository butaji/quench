// Layout corpus: allocation churn that runs young and full collections, finalization and weak
// references, the collector's code paths every long-running program reaches (no scored suite
// code). Prints one checksum.
"use strict";
class Node { constructor(value, left, right) { this.value = value; this.left = left; this.right = right; } }
function tree(depth) { return depth === 0 ? new Node(depth, null, null) : new Node(depth, tree(depth - 1), tree(depth - 1)); }
function count(node) { return node === null ? 0 : 1 + count(node.left) + count(node.right); }
const registry = new FinalizationRegistry(() => {});
const weak = new WeakMap();
const refs = [];
let long = [];
let sum = 0;
for (let round = 0; round < 60; round++) {
    const t = tree(10);
    sum += count(t);
    const objects = [];
    for (let i = 0; i < 3000; i++) {
        const o = { i, s: "k" + i, a: [i, i + 1], f: () => i + round };
        objects.push(o);
        if (i % 97 === 0) { weak.set(o, i); registry.register(o, i); refs.push(new WeakRef(o)); }
    }
    sum += objects.filter((o) => o.i % 2).map((o) => o.f()).length;
    long.push(objects.slice(0, 200), new Float64Array(256), "x".repeat(round + 10).split(""));
    if (long.length > 60) long = long.slice(30);
    const m = new Map();
    for (let i = 0; i < 500; i++) m.set("key" + (i % 250), { i });
    sum += m.size;
}
sum += refs.filter((r) => r.deref() !== undefined).length >= 0 ? 1 : 0;
console.log("rt-gc", sum, long.length);
