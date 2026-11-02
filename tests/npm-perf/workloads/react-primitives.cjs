// Guest workloads isolating operations used by React's server renderer.
const width = 128;
const pieces = ["article", " class=", "card", "odd", "data-index", "text", "value", "end"];
const pieceMask = pieces.length - 1;
const records = Array.from({ length: width }, (_, index) => ({ a: index, b: index + 1, c: index + 2, d: index + 3 }));
const props = { className: "card", id: "entry", title: "title", children: "content" };
const propKeys = Object.keys(props);
const propKeyMask = propKeys.length - 1;
const hasOwn = Object.prototype.hasOwnProperty;
const special = /["'&<>]/;
const texts = ["a plain paragraph", "class-name", "data-index", "text", "a&b", "<entry>", 'a"b', "a'b"];
const textMask = texts.length - 1;
const state = { a: 0, b: 0, retained: null };
function four(a, b, c, d) { return a + b + c + d; }
const operations = {
  "own-check": function () {
    let sum = 0;
    for (let index = 0; index < width; index++) sum += hasOwn.call(props, propKeys[index & propKeyMask]) ? 1 : 0;
    return sum;
  },
  "own-read": function () {
    let sum = 0;
    for (let index = 0; index < width; index++) sum += records[index].a + records[index].c;
    return sum;
  },
  "own-write": function () {
    let sum = 0;
    for (let index = 0; index < width; index++) {
      state.a = index;
      state.b = index + 1;
      sum += state.a + state.b;
    }
    return sum;
  },
  "props-walk": function () {
    let sum = 0;
    for (let index = 0; index < width; index++) {
      for (const key in props) if (hasOwn.call(props, key)) sum += props[key].length;
    }
    return sum;
  },
  "record-allocate": function () {
    const batch = [];
    for (let index = 0; index < width; index++) batch.push({ tag: 5, index, text: pieces[index & pieceMask], next: null });
    state.retained = batch;
    let sum = 0;
    for (let index = 0; index < width; index++) sum += batch[index].index;
    return sum;
  },
  "text-append": function () {
    let text = "";
    for (let index = 0; index < width; index++) text += pieces[index & pieceMask];
    return text.length + text.charCodeAt(text.length - 1);
  },
  "chunks-join": function () {
    const chunks = [];
    for (let index = 0; index < width; index++) chunks.push(pieces[index & pieceMask]);
    const text = chunks.join("");
    return text.length + text.charCodeAt(text.length - 1);
  },
  "escape-scan": function () {
    let sum = 0;
    for (let index = 0; index < width; index++) {
      const match = special.exec(texts[index & textMask]);
      sum += match === null ? texts[index & textMask].length : match.index;
    }
    return sum;
  },
  "call-four": function () {
    let sum = 0;
    for (let index = 0; index < width; index++) sum += four(index, index + 1, index + 2, index + 3);
    return sum;
  },
};
const primitive = process.env.QUENCH_PRIMITIVE || "own-read";
const operation = operations[primitive];
if (typeof operation !== "function") throw new Error("unknown primitive: " + primitive);
const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 10000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 25);
for (let index = 0; index < warmups; index++) operation();
let checksum = 0;
const start = Date.now();
for (let index = 0; index < rounds; index++) checksum = (checksum + operation()) >>> 0;
const elapsedMs = Math.max(1, Date.now() - start);
console.log(JSON.stringify({ workload: "react-primitives", primitive, rounds, warmups, operations: rounds * width, checksum, elapsedMs, operationsPerSecond: rounds * width * 1000 / elapsedMs }));
