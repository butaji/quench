#!/usr/bin/env node
/**
 * Generate the VM micro-corpus.
 *
 * The case descriptions live here once.  The numbered files are deliberately
 * boring, standalone JavaScript programs so that any engine can execute them
 * without a harness-specific API.
 */
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = dirname(fileURLToPath(import.meta.url));
export const CASE_COUNT = 500;
export const CASES_PER_FAMILY = 50;

export const families = [
  ["primitives", "numbers, coercion, equality, and primitive values"],
  ["control-flow", "branches, loops, switch, and abrupt completion"],
  ["arrays", "dense and sparse arrays plus array algorithms"],
  ["objects", "shapes, descriptors, prototypes, accessors, and symbols"],
  ["functions", "closures, calls, recursion, constructors, and classes"],
  ["strings-regexp", "unicode strings, templates, regular expressions, and text"],
  ["iterables", "iterators, generators, destructuring, and spread"],
  ["collections", "Map, Set, WeakMap, and object-key identity"],
  ["typed-memory", "ArrayBuffer, typed arrays, DataView, and BigInt values"],
  ["meta-builtins", "Proxy, Reflect, JSON, Date, errors, and eval"],
];

const header = (id, family, depth) => `// VM micro-case ${String(id).padStart(3, "0")}\n// family=${family}; level=${Math.floor((id - 1) / CASES_PER_FAMILY) + 1}; depth=${depth}\n`;

const prelude = `"use strict";\nconst assert = (condition, message) => {\n  if (!condition) throw new Error("micro assertion failed: " + message);\n};\nconst same = (a, b) => Object.is(a, b);\n`;

// Bounded, representative kernels borrowed from the shapes of classic
// dynamic-language workloads (numeric relaxation, DeltaBlue-style constraints,
// Richards-style scheduling, lexer/parser work, and hash/tree lookup). They
// use ordinary ECMAScript data and are intentionally independent of filenames.
function workloadBody(depth, mode) {
  if (mode === 5) return `const n = ${Math.max(4, depth * 3)}; let a = Array.from({ length: n }, (_, i) => (i % 7) / 7);\nfor (let pass = 0; pass < 4; pass++) for (let i = 1; i < n - 1; i++) a[i] = (a[i - 1] + a[i] + a[i + 1]) / 3;\nassert(a.every(Number.isFinite), "numeric relaxation");\nreturn Number(a.reduce((sum, value) => sum + value, 0).toFixed(6));`;
  if (mode === 6) return `const nodes = Array.from({ length: ${Math.max(3, depth % 17 + 3)} }, (_, id) => ({ id, value: id }));\nfor (let pass = 0; pass < ${Math.max(2, depth % 8)}; pass++) for (let i = 1; i < nodes.length; i++) nodes[i].value = nodes[i - 1].value + 1;\nassert(nodes.at(-1).value === nodes.length - 1, "constraint propagation");\nreturn nodes.at(-1).value;`;
  if (mode === 7) return `const queue = Array.from({ length: ${Math.max(3, depth % 13 + 3)} }, (_, id) => ({ id, budget: id % 4 + 1 })); let ticks = 0;\nwhile (queue.length) { const task = queue.shift(); ticks += task.budget; if (task.budget > 1) queue.push({ id: task.id, budget: task.budget - 1 }); }\nassert(ticks > 0 && queue.length === 0, "scheduler queue");\nreturn ticks;`;
  if (mode === 8) return `const source = Array.from({ length: ${Math.max(4, depth * 2)} }, (_, i) => (i % 2 ? "word" : "number") + i).join(" "); const tokens = source.match(/[a-z]+|\\d+/gi) || [];\nassert(tokens.length === ${Math.max(4, depth * 2) * 2}, "lexer tokens");\nreturn tokens.slice(0, 6);`;
  return `const table = new Map(); let hash = 2166136261; for (let i = 0; i < ${Math.max(4, depth * 2)}; i++) { hash ^= (i * 2654435761) >>> 0; hash = Math.imul(hash, 16777619) >>> 0; table.set(i, hash); }\nlet found = 0; for (let i = ${Math.max(4, depth * 2) - 1}; i >= 0; i--) if (table.has(i)) found += table.get(i) & 1;\nassert(table.size === ${Math.max(4, depth * 2)} && found >= 0, "hash lookup");\nreturn [table.size, found, hash];`;
}

function primitiveBody(depth, mode) {
  if (mode === 0) return `let total = 0;\nfor (let i = 0; i < ${depth}; i++) total = (total + i * 3 + 1) % 997;\nassert(total === (${depth} * (${depth} - 1) * 3 / 2 + ${depth}) % 997, "arithmetic");\nreturn { total, inf: 1 / 0, nan: Number.isNaN(0 / 0) };`;
  if (mode === 1) return `const values = ["${depth}", ${depth}, true, null];\nconst coerced = values.map(Number);\nassert(coerced[0] === ${depth} && coerced[1] === ${depth} && coerced[2] === 1 && coerced[3] === 0, "coercion");\nreturn coerced;`;
  if (mode === 2) return `let bits = ${depth};\nfor (let i = 0; i < ${depth % 9 + 1}; i++) bits = ((bits << 3) ^ (bits >>> 2) ^ i) | 0;\nassert(Number.isInteger(bits), "bitwise integer");\nreturn bits;`;
  if (mode === 3) return `const negativeZero = -0;\nconst values = [negativeZero, Number.MIN_VALUE, Number.MAX_SAFE_INTEGER, BigInt(${depth})];\nassert(same(values[0], -0) && values[1] > 0 && Number.isSafeInteger(values[2]), "numeric edges");\nreturn [Object.is(values[0], -0), String(values[3])];`;
  return `const a = ${depth};\nconst b = Number(String(a));\nassert(a == b && a === b && !same(a, -a - 1), "equality");\nreturn { loose: a == b, strict: a === b, type: typeof a };`;
}

function controlBody(depth, mode) {
  if (mode === 0) return `let sum = 0;\nfor (let i = 0; i < ${depth}; i++) { if (i % 2 === 0) sum += i; else sum -= i; }\nassert(sum === ${depth % 2 === 0 ? -(depth / 2) : (depth - 1) / 2}, "branch loop");\nreturn sum;`;
  if (mode === 1) return `let cells = 0;\nfor (let row = 0; row < ${Math.max(1, depth % 8)}; row++) for (let col = 0; col < ${Math.max(1, depth % 7)}; col++) cells += row + col;\nassert(cells >= 0, "nested loop");\nreturn cells;`;
  if (mode === 2) return `let label = "";\nfor (let i = 0; i < ${depth}; i++) { switch (i % 3) { case 0: label += "a"; break; case 1: label += "b"; break; default: label += "c"; } }\nassert(label.length === ${depth}, "switch");\nreturn label;`;
  if (mode === 3) return `let state = 0;\ntry { for (let i = 0; i < ${depth}; i++) { state += i; if (i === ${Math.max(0, depth - 2)}) throw new RangeError("stop"); } } catch (error) { assert(error instanceof RangeError, "catch type"); state += 1; } finally { state *= 2; }\nreturn state;`;
  return `let kept = [];\nouter: for (let i = 0; i < ${depth}; i++) { for (let j = 0; j < 4; j++) { if ((i + j) % 3 === 0) continue outer; } kept.push(i); }\nassert(kept.every((x) => x % 3 !== 0), "labeled continue");\nreturn kept.length;`;
}

function arrayBody(depth, mode) {
  if (mode === 0) return `const input = Array.from({ length: ${depth}, }, (_, i) => i + 1);\nconst output = input.map((x) => x * 2).filter((x) => x % 3 !== 0);\nassert(input.length === ${depth} && output.every((x) => x % 2 === 0), "map filter");\nreturn output.reduce((a, x) => a + x, 0);`;
  if (mode === 1) return `const sparse = []; sparse[${Math.max(0, depth - 1)}] = ${depth};\nconst seen = []; sparse.forEach((x) => seen.push(x));\nassert(sparse.length === ${depth} && seen.length === 1 && !(0 in sparse), "holes");\nreturn [Object.keys(sparse), seen];`;
  if (mode === 2) return `const list = [1, 2, 3];\nfor (let i = 0; i < ${depth % 11}; i++) list.push(i);\nconst last = list.pop(); list.unshift(last);\nassert(list[0] === last && list.length === 3 + ${depth % 11}, "mutations");\nreturn list.slice(0, 5);`;
  if (mode === 3) return `const values = Array.from({ length: ${Math.max(1, depth % 10)} }, (_, i) => (${depth} * 7 + i * 11) % 101);\nvalues.sort((a, b) => a - b);\nassert(values.every((x, i) => i === 0 || values[i - 1] <= x), "sort comparator");\nreturn values;`;
  return `const nested = Array.from({ length: ${Math.max(1, depth % 7)} }, (_, i) => [i, [i + 1]]);\nconst flat = nested.flat(2).flatMap((x) => [x, x]);\nassert(flat.length === nested.length * 4, "flat map");\nreturn flat;`;
}

function objectBody(depth, mode) {
  if (mode === 0) return `const object = {}; object.z = 1; object["a"] = 2; object[${depth}] = 3;\nconst keys = Reflect.ownKeys(object);\nassert(keys[0] === String(${depth}) && keys[1] === "z" && keys[2] === "a", "property order");\nreturn keys;`;
  if (mode === 1) return `const object = {}; Object.defineProperty(object, "answer", { value: ${depth}, enumerable: false, writable: false, configurable: true });\nconst descriptor = Object.getOwnPropertyDescriptor(object, "answer");\nassert(descriptor.value === ${depth} && descriptor.enumerable === false && descriptor.writable === false, "descriptor");\nreturn [Object.keys(object), descriptor.value];`;
  if (mode === 2) return `const parent = { base: ${depth} }; const child = Object.create(parent); child.own = 2;\nassert(child.base === ${depth} && Object.hasOwn(child, "own") && !Object.hasOwn(child, "base"), "prototype");\nreturn [child.base, Object.getPrototypeOf(child) === parent];`;
  if (mode === 3) return `let stored = 0; const object = { get value() { return stored; }, set value(next) { stored = next * 2; } }; object.value = ${depth};\nassert(object.value === ${depth * 2} && stored === ${depth * 2}, "accessor");\nreturn object.value;`;
  return `const key = Symbol("micro"); const object = { [key]: ${depth}, plain: true };\nassert(object[key] === ${depth} && Reflect.ownKeys(object).includes(key), "symbol key");\nreturn [typeof key, object.plain, object[key]];`;
}

function functionBody(depth, mode) {
  if (mode === 0) return `function makeCounter(start) { let value = start; return () => ++value; }\nconst counter = makeCounter(${depth}); const values = [counter(), counter(), counter()];\nassert(values[2] === ${depth + 3} && values[0] < values[1], "closure");\nreturn values;`;
  if (mode === 1) return `function factorial(n) { return n <= 1 ? 1 : n * factorial(n - 1); }\nconst value = factorial(${Math.min(depth % 9, 8)});\nassert(Number.isInteger(value) && value > 0, "recursion");\nreturn value;`;
  if (mode === 2) return `function add(a, b) { return this.bias + a + b; }\nconst receiver = { bias: ${depth} }; const value = add.call(receiver, 2, 3) + add.apply(receiver, [4, 5]) + add.bind(receiver, 6)(7);\nassert(value === ${3 * depth + 27}, "call apply bind");\nreturn value;`;
  if (mode === 3) return `function Box(value) { this.value = value; }\nBox.prototype.bump = function () { return ++this.value; }; const box = new Box(${depth});\nassert(box instanceof Box && box.bump() === ${depth + 1}, "constructor prototype");\nreturn box.value;`;
  return `class Box { constructor(value) { this.value = value; } bump(amount = 1) { return this.value + amount; } static label() { return "Box"; } }\nconst box = new Box(${depth});\nassert(box instanceof Box && box.bump(${depth % 5}) === ${depth + depth % 5} && Box.label() === "Box", "class");\nreturn [box.value, Box.label()];`;
}

function stringBody(depth, mode) {
  if (mode === 0) return `const text = "micro-${depth}-\\u{1F600}"; const points = [...text];\nassert(points.at(-1) === "😀" && text.includes(String(${depth})), "unicode code points");\nreturn [text.length, points.length];`;
  if (mode === 1) return "const tag = (parts, ...values) => parts.reduce((out, part, i) => out + part + (values[i] ?? \"\"), \"\");\nconst text = tag`case-" + depth + "-${" + depth + " * 2}`;\nassert(text === \"case-" + depth + "-" + depth * 2 + "\", \"template tag\");\nreturn text;";
  if (mode === 2) return `const re = /[a-z]+/gi; const matches = "a${depth}bb Ccc".match(re);\nassert(matches.length === 3 && matches[0].toLowerCase() === "a", "regexp match");\nreturn matches;`;
  if (mode === 3) return `const text = "a-b-c-${depth}"; const changed = text.replaceAll("-", ":"); const parts = changed.split(":");\nassert(parts.length === 4 && parts.at(-1) === String(${depth}), "replace split");\nreturn parts;`;
  return `const composed = "e\\u0301"; const normalized = composed.normalize("NFC");\nassert(normalized === "é" && normalized.normalize("NFD").length >= 2, "normalization");\nreturn normalized;`;
}

function iterableBody(depth, mode) {
  if (mode === 0) return `const iterable = { [Symbol.iterator]() { let i = 0; return { next() { return i < ${depth % 9 + 1} ? { value: i++, done: false } : { value: undefined, done: true }; } }; } };\nconst values = [...iterable];\nassert(values.length === ${depth % 9 + 1} && values[0] === 0, "iterator protocol");\nreturn values;`;
  if (mode === 1) return `function* sequence() { for (let i = 0; i < ${depth % 9 + 1}; i++) yield i * i; }\nconst values = [...sequence()];\nassert(values.at(-1) === (${depth % 9}) ** 2, "generator");\nreturn values;`;
  if (mode === 2) return `const source = [1, 2, 3, 4, 5]; const [first, ...middle] = source; const [a, , c] = middle;\nassert(first === 1 && a === 2 && c === 4 && middle.length === 4, "destructuring");\nreturn [first, ...middle.slice(0, ${Math.max(1, depth % 5)})];`;
  if (mode === 3) return `const left = { x: ${depth}, y: 2 }; const right = { y: 3, z: 4 }; const merged = { ...left, ...right };\nassert(merged.x === ${depth} && merged.y === 3 && merged.z === 4, "object spread");\nreturn Object.keys(merged);`;
  return `const values = []; for (const value of new Set([${depth}, ${depth}, ${depth + 1}])) values.push(value);\nassert(values.length === 2 && values[0] === ${depth}, "for of");\nreturn values;`;
}

function collectionBody(depth, mode) {
  if (mode === 0) return `const map = new Map(); map.set("first", ${depth}); map.set("second", ${depth + 1}); map.set("first", ${depth + 2});\nassert(map.size === 2 && map.get("first") === ${depth + 2}, "map identity");\nreturn [...map.keys()];`;
  if (mode === 1) return `const set = new Set([1, 1, 2, ${depth % 3}, ${depth % 3}]); set.add(3);\nassert(set.has(1) && set.size <= 4, "set uniqueness");\nreturn [...set].sort((a, b) => a - b);`;
  if (mode === 2) return `const key = {}; const weak = new WeakMap([[key, ${depth}]]);\nassert(weak.has(key) && weak.get(key) === ${depth}, "weak map key");\nreturn weak.get(key);`;
  if (mode === 3) return `const first = {}; const second = {}; const map = new Map([[first, "first"], [second, "second"]]);\nassert(map.get(first) === "first" && map.get(second) === "second" && map.get({}) === undefined, "object key identity");\nreturn map.size;`;
  return `const map = new Map(Array.from({ length: ${Math.max(1, depth % 8)} }, (_, i) => [i, i * i])); let total = 0; for (const [key, value] of map) total += key + value;\nassert(total >= 0 && map.size === ${Math.max(1, depth % 8)}, "map iteration");\nreturn total;`;
}

function typedBody(depth, mode) {
  if (mode === 0) return `const bytes = new Uint8Array(${Math.max(2, depth % 16 + 2)}); for (let i = 0; i < bytes.length; i++) bytes[i] = i * 17;\nassert(bytes[0] === 0 && bytes[1] === 17, "uint8");\nreturn [...bytes.slice(0, 4)];`;
  if (mode === 1) return `const buffer = new ArrayBuffer(8); const view = new DataView(buffer); view.setInt32(0, ${depth}, true); view.setInt32(4, -${depth}, false);\nassert(view.getInt32(0, true) === ${depth} && view.getInt32(4, false) === -${depth}, "dataview endian");\nreturn [view.getInt32(0, true), view.getInt32(4, false)];`;
  if (mode === 2) return `const values = new Float64Array([${depth}.5, NaN, -0]);\nassert(values[0] === ${depth}.5 && Number.isNaN(values[1]) && Object.is(values[2], -0), "float64");\nreturn [values[0], Number.isNaN(values[1]), Object.is(values[2], -0)];`;
  if (mode === 3) return `const values = new BigInt64Array([BigInt(${depth}), -BigInt(${depth})]);\nassert(values[0] === BigInt(${depth}) && values[1] === -BigInt(${depth}), "bigint typed array");\nreturn Array.from(values, String);`;
  return `const original = new Uint16Array([${depth}, ${depth + 1}, ${depth + 2}]); const copy = original.buffer.slice(2); const sliced = new Uint16Array(copy);\nassert(sliced[0] === ${depth + 1} && sliced.length === 2, "buffer slice");\nreturn [...sliced];`;
}

function metaBody(depth, mode) {
  if (mode === 0) return `const events = []; const target = { value: ${depth} }; const proxy = new Proxy(target, { get(object, key, receiver) { events.push(String(key)); return Reflect.get(object, key, receiver); } });\nassert(proxy.value === ${depth} && events[0] === "value", "proxy get");\nreturn events;`;
  if (mode === 1) return `const object = {}; Reflect.defineProperty(object, "value", { value: ${depth}, enumerable: true });\nassert(Reflect.has(object, "value") && Reflect.get(object, "value") === ${depth}, "reflect");\nreturn Reflect.ownKeys(object);`;
  if (mode === 2) return `const source = { id: ${depth}, values: [1, 2, 3] }; const roundTrip = JSON.parse(JSON.stringify(source));\nassert(roundTrip.id === ${depth} && roundTrip.values.join(",") === "1,2,3", "json");\nreturn roundTrip;`;
  if (mode === 3) return `const date = new Date(Date.UTC(2000 + (${depth} % 20), ${depth} % 12, 1));\nassert(date.getUTCFullYear() === 2000 + (${depth} % 20) && date.getUTCMonth() === ${depth} % 12, "date utc");\nreturn date.toISOString().slice(0, 10);`;
  return `let message = ""; try { throw new TypeError("micro-${depth}"); } catch (error) { assert(error instanceof TypeError && error.message === "micro-${depth}", "error object"); message = error.name + ":" + error.message; }\nconst evaluated = Function("x", "return x + 1")(${depth});\nassert(evaluated === ${depth + 1}, "function constructor");\nreturn message;`;
}

const builders = [
  primitiveBody,
  controlBody,
  arrayBody,
  objectBody,
  functionBody,
  stringBody,
  iterableBody,
  collectionBody,
  typedBody,
  metaBody,
];

export function renderCase(id) {
  if (!Number.isInteger(id) || id < 1 || id > CASE_COUNT) throw new RangeError(`invalid micro id: ${id}`);
  const familyIndex = Math.floor((id - 1) / CASES_PER_FAMILY);
  const depth = ((id - 1) % CASES_PER_FAMILY) + 1;
  const [family] = families[familyIndex];
  const mode = (depth - 1) % 10;
  const body = mode < 5 ? builders[familyIndex](depth, mode) : workloadBody(depth, mode);
  return `${header(id, family, depth)}${prelude}const result = (() => {\n${body}\n})();\nconst emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});\nemit("ok:" + JSON.stringify(result));\n`;
}

export function manifest() {
  return {
    schema: 1,
    count: CASE_COUNT,
    numbering: "001.js..500.js",
    complexity: "Ten families of 50 cases. Within each family depth increases from 1 to 50; the first five cases of every ten rotate semantic properties, and the other five add a bounded classic workload kernel.",
    families: families.map(([name, description], index) => ({
      first: index * CASES_PER_FAMILY + 1,
      last: (index + 1) * CASES_PER_FAMILY,
      name,
      description,
    })),
    oracle: "Each case is a standalone ECMAScript program. A zero exit status and matching generic ok: JSON output are required.",
    scoring: "V8-style geometric mean: speed_score = 100 * GM(oracle wall_ns / engine wall_ns); memory_score = 100 * GM(oracle peak RSS / engine peak RSS); overall_score = GM of available component scores. Scores are versioned and only comparable within this corpus version and machine.",
    design_neutrality: "Cases use public ECMAScript behavior only. They do not name opcodes, tags, shapes, dispatch strategies, collectors, or VM internals; any implementation that preserves observable semantics can benefit.",
    workload_bias: "General-purpose JavaScript: no DOM, browser, Node-only module, network, filesystem, or web-framework dependency.",
    classic_sources: [
      "js-engine-benchmark/v8-v7/richards.js",
      "js-engine-benchmark/v8-v7/deltablue.js",
      "js-engine-benchmark/v8-v7/crypto.js",
      "js-engine-benchmark/v8-v7/raytrace.js",
      "js-engine-benchmark/v8-v7/earley-boyer.js",
      "js-engine-benchmark/v8-v7/regexp.js",
      "js-engine-benchmark/v8-v7/splay.js",
      "js-engine-benchmark/v8-v7/navier-stokes.js",
    ],
    confidence: "This corpus is evidence for VM tuning, not a proof of V8 equivalence. Differential runs against Node plus test262 remain required for a compatibility claim.",
  };
}

function expectedNames() {
  return Array.from({ length: CASE_COUNT }, (_, index) => `${String(index + 1).padStart(3, "0")}.js`);
}

export function checkCorpus() {
  const expected = expectedNames();
  const actual = readdirSync(ROOT).filter((name) => /^\d{3}\.js$/.test(name)).sort();
  const errors = [];
  if (actual.length !== expected.length) errors.push(`expected ${expected.length} numbered scripts, found ${actual.length}`);
  for (let index = 0; index < expected.length; index++) {
    const name = expected[index];
    if (actual[index] !== name) errors.push(`missing or unexpected script at ${name}: ${actual[index] ?? "<none>"}`);
    else if (readFileSync(join(ROOT, name), "utf8") !== renderCase(index + 1)) errors.push(`${name} is stale; run node micros/generate.mjs`);
  }
  return errors;
}

const invoked = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (invoked) {
  const check = process.argv.includes("--check");
  if (check) {
    const errors = checkCorpus();
    if (errors.length) { console.error(errors.join("\n")); process.exit(1); }
    console.log(`micro corpus is complete: ${CASE_COUNT} scripts`);
  } else {
    mkdirSync(ROOT, { recursive: true });
    for (let id = 1; id <= CASE_COUNT; id++) writeFileSync(join(ROOT, `${String(id).padStart(3, "0")}.js`), renderCase(id));
    writeFileSync(join(ROOT, "manifest.json"), JSON.stringify(manifest(), null, 2) + "\n");
    console.log(`generated ${CASE_COUNT} scripts in ${ROOT}`);
  }
}
