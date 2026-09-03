// Linked-list and tree traversal
// stage=full-system-closure; mechanism=Object-oriented traversal over a small tree and a linked list, bounded shape set, hot enough to tier up.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Node(value, left, right) { this.value = value; this.left = left; this.right = right; }
  function buildTree(depth, seed) {
    if (depth === 0) return null;
    return new Node(seed, buildTree(depth - 1, seed * 2 + 1), buildTree(depth - 1, seed * 2 + 2));
  }
  function sumTree(node) {
    if (node === null) return 0;
    return node.value + sumTree(node.left) + sumTree(node.right);
  }
  const tree = buildTree(12, 1);
  let total = 0;
  for (let iter = 0; iter < 20; iter++) total += sumTree(tree);
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
