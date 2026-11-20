"use strict";

// Equal edge signatures still require full contents and first-source-case ordering.
function choose(value) {
  switch (value) {
    case "": return 0;
    case "\u0000": return 1;
    case "alpha": return 2;
    case "alpHa": return 3;
    case "abca": return 4;
    case "acba": return 5;
    case "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa": return 9;
    case "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": return 63;
    case "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": return 64;
    case "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx": return 65;
    case "same": return 6;
    case "same": return 99;
    case "\ud800x": return 7;
    case "\ud800yx": return 8;
    default: return -1;
  }
}

// More than eight equal signatures must retain the exact general selector.
function collisionOverflow(value) {
  switch (value) {
    case "a00a": return 0;
    case "a01a": return 1;
    case "a02a": return 2;
    case "a03a": return 3;
    case "a04a": return 4;
    case "a05a": return 5;
    case "a06a": return 6;
    case "a07a": return 7;
    case "a08a": return 8;
    default: return -1;
  }
}

const inputs = [
  "", "\0", "alpha", "alpHa", "abca", "acba", "aaaa", "same",
  "\ud800x", "\ud800yx", "tail", null, undefined, 42, 42n, Symbol("alpha"),
  // An ASCII slice of wide storage, and lengths around the u64 mask boundary.
  ("你好" + "a".repeat(40)).slice(2),
  "x".repeat(63), "x".repeat(64), "x".repeat(65),
  new String("alpha"),
  { toString() { throw Error("unexpected coercion"); } },
];
const out = [];
for (let round = 0; round < 100; round++) {
  for (const value of inputs) out.push(choose(value));
}

let rope = "a";
for (let i = 0; i < 1000; i++) rope += "b";
out.push(choose(rope), choose(rope.slice(0, 4) + "a"));

// Filters retain immutable constant identities across collection, not guest objects.
for (let round = 0; round < 15; round++) {
  const pressure = [];
  for (let i = 0; i < 3000; i++) pressure.push({ a: i, b: "key" + i });
  for (const value of inputs) out.push(choose(value));
}
for (let round = 0; round < 30; round++) {
  for (let i = 0; i < 9; i++) out.push(collisionOverflow("a0" + i + "a"));
  out.push(collisionOverflow("a99a"));
}
console.log(JSON.stringify(out));
