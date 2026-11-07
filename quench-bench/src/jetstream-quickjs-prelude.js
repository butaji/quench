// JetStream shell surface on QuickJS (`qjs --std`). argv: <prelude> <jetstream-root> <cli args...>
// QuickJS has no realm API, so `runString` evaluates the test's scripts in this realm: every
// test runs alone in a fresh process, and the driver only reaches the test through the global
// it returns. The driver itself (cli.js and the files it loads at its top level) runs inside one
// function scope, so its declarations (`class Benchmark`, ...) leave the global to the test. QuickJS has no zlib either; `inflate` decodes the corpus's `.z` resources.
"use strict";
const root = scriptArgs[1];
const resolve = (file) => (file.startsWith("/") ? file : `${root}/${file}`);
const exists = (file) => os.stat(file)[1] === 0;
const readBytes = (file) => {
  const handle = std.open(file, "rb");
  if (!handle) throw new Error(`cannot open ${file}`);
  handle.seek(0, std.SEEK_END);
  const size = handle.tell();
  handle.seek(0, std.SEEK_SET);
  const bytes = new Uint8Array(size);
  handle.read(bytes.buffer, 0, size);
  handle.close();
  return bytes;
};
// RFC 1950/1951 inflate of a zlib stream.
function inflate(input) {
  let position = 2, bitBuffer = 0, bitCount = 0;
  const output = [];
  const bits = (count) => {
    while (bitCount < count) { bitBuffer |= input[position++] << bitCount; bitCount += 8; }
    const value = bitBuffer & ((1 << count) - 1);
    bitBuffer >>>= count; bitCount -= count;
    return value;
  };
  const table = (lengths) => {
    const counts = new Uint16Array(16), symbols = new Uint16Array(lengths.length), offsets = new Uint16Array(16);
    for (const length of lengths) counts[length]++;
    counts[0] = 0;
    for (let length = 1; length < 16; length++) offsets[length] = offsets[length - 1] + counts[length - 1];
    lengths.forEach((length, symbol) => { if (length) symbols[offsets[length]++] = symbol; });
    return { counts, symbols };
  };
  const decode = ({ counts, symbols }) => {
    let code = 0, first = 0, index = 0;
    for (let length = 1; length < 16; length++) {
      code |= bits(1);
      const count = counts[length];
      if (code - count < first) return symbols[index + (code - first)];
      index += count; first += count; first <<= 1; code <<= 1;
    }
    throw new Error("bad inflate code");
  };
  const lengthBase = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
  const lengthExtra = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
  const distanceBase = [1,2,3,4,5,7,9,13,17,25,33,49,65,97,129,193,257,385,513,769,1025,1537,2049,3073,4097,6145,8193,12289,16385,24577];
  const distanceExtra = [0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13];
  const fixedLiterals = table(Array.from({ length: 288 }, (_, symbol) => symbol < 144 ? 8 : symbol < 256 ? 9 : symbol < 280 ? 7 : 8));
  const fixedDistances = table(new Array(30).fill(5));
  const order = [16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15];
  let last;
  do {
    last = bits(1);
    const type = bits(2);
    if (type === 0) {
      bitBuffer = 0; bitCount = 0;
      const length = input[position] | (input[position + 1] << 8);
      position += 4;
      for (let index = 0; index < length; index++) output.push(input[position++]);
      continue;
    }
    let literals = fixedLiterals, distances = fixedDistances;
    if (type === 2) {
      const literalCount = bits(5) + 257, distanceCount = bits(5) + 1, codeCount = bits(4) + 4;
      const codeLengths = new Array(19).fill(0);
      for (let index = 0; index < codeCount; index++) codeLengths[order[index]] = bits(3);
      const codes = table(codeLengths);
      const lengths = [];
      while (lengths.length < literalCount + distanceCount) {
        const symbol = decode(codes);
        if (symbol < 16) lengths.push(symbol);
        else if (symbol === 16) { const previous = lengths[lengths.length - 1]; for (let repeat = bits(2) + 3; repeat; repeat--) lengths.push(previous); }
        else if (symbol === 17) for (let repeat = bits(3) + 3; repeat; repeat--) lengths.push(0);
        else for (let repeat = bits(7) + 11; repeat; repeat--) lengths.push(0);
      }
      literals = table(lengths.slice(0, literalCount));
      distances = table(lengths.slice(literalCount));
    }
    for (;;) {
      const symbol = decode(literals);
      if (symbol < 256) { output.push(symbol); continue; }
      if (symbol === 256) break;
      const lengthSymbol = symbol - 257;
      const length = lengthBase[lengthSymbol] + bits(lengthExtra[lengthSymbol]);
      const distanceSymbol = decode(distances);
      const distance = distanceBase[distanceSymbol] + bits(distanceExtra[distanceSymbol]);
      for (let index = 0; index < length; index++) output.push(output[output.length - distance]);
    }
  } while (!last);
  return Uint8Array.from(output);
}
const utf8 = (bytes) => {
  let text = "";
  const chunk = 0x8000;
  for (let start = 0; start < bytes.length; start += chunk)
    text += String.fromCharCode.apply(null, bytes.subarray(start, start + chunk));
  return decodeURIComponent(escape(text));
};
const readFile = (file, mode) => {
  const location = resolve(file);
  const bytes = !exists(location) && exists(`${location}.z`) ? inflate(readBytes(`${location}.z`)) : readBytes(location);
  return mode === "binary" ? bytes : utf8(bytes);
};
const print = (...args) => { std.out.puts(args.join(" ") + "\n"); std.out.flush(); };
const printErr = (...args) => { std.err.puts(args.join(" ") + "\n"); std.err.flush(); };
const evaluate = (source) => std.evalScript(source);
globalThis.print = print;
globalThis.printErr = printErr;
globalThis.console = { log: print, info: print, warn: printErr, error: printErr, debug: print, assert(condition, ...args) { if (!condition) printErr("Assertion failed:", ...args); } };
globalThis.read = readFile;
globalThis.readFile = readFile;
globalThis.load = (file) => evaluate(readFile(file));
globalThis.loadString = (source) => evaluate(source);
globalThis.runString = (source) => { evaluate(source); return globalThis; };
globalThis.setTimeout = (callback, delay = 0, ...args) => os.setTimeout(() => callback(...args), delay);
globalThis.clearTimeout = (timer) => os.clearTimeout(timer);
globalThis.queueMicrotask = (callback) => { Promise.resolve().then(callback); };
const origin = Date.now();
globalThis.performance = { timeOrigin: origin, now: () => Date.now() - origin };
globalThis.arguments = scriptArgs.slice(2);
const driverLoad = /^load\("([^"]+)"\);$/gm;
const driverSource = (file) => readFile(file).replace(driverLoad, (_, inner) => driverSource(inner));
evaluate(`(function () {\n${driverSource("cli.js")}\n})();`);
