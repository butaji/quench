// Instrumented node run of the JetStream pdfjs row (Octane/pdfjs.js).
// Prints the expected validation values and per-iteration work counts that the
// C analogue (quench-bench/c-analogues/pdfjs.c) must reproduce.
// usage: node pdfjs-validate.cjs [JETSTREAM_ROOT] [ITERATIONS]
'use strict';
const vm = require('vm');
const fs = require('fs');
const path = require('path');

const root = process.argv[2] || path.join(__dirname, '../../../../../quench/quench-bench/jetstream');
const iterations = +(process.argv[3] || 1);
let src = fs.readFileSync(path.join(root, 'Octane/pdfjs.js'), 'utf8');

// Instrumentation points (text patches; the original file is not modified).
const patches = [
  ['fnArray.push(fn);', 'fnArray.push(fn); __count.ops[fn] = (__count.ops[fn] || 0) + 1;'],
  ['getObj: function Parser_getObj(cipherTransform) {',
   'getObj: function Parser_getObj(cipherTransform) { __count.parserGetObj++;'],
  ['getChar: function Stream_getChar() {', 'getChar: function Stream_getChar() { __count.streamGetChar++;'],
  ['function FlateStream(stream) {', 'function FlateStream(stream) { __count.flate.push(this);'],
  ['function Font(name, file, properties) {', 'function Font(name, file, properties) { __count.fonts.push(this);'],
  ['charToGlyph: function Font_charToGlyph(charcode) {',
   'charToGlyph: function Font_charToGlyph(charcode) { __count.charToGlyph++;'],
  ['charsToGlyphs: function Font_charsToGlyphs(chars) {',
   'charsToGlyphs: function Font_charsToGlyphs(chars) { __count.charsToGlyphs++;'],
  ['this.charstrings = charstrings;', 'this.charstrings = charstrings; __count.charstrings += charstrings.length;'],
  ['function decodeCharString(array) {', 'function decodeCharString(array) { __count.decodeCharString++;'],
];
for (const [a, b] of patches) {
  const n = src.split(a).length - 1;
  if (n < 1) throw new Error('patch anchor not found: ' + a);
  src = src.split(a).join(b);
}

const __count = { ops: {}, parserGetObj: 0, streamGetChar: 0, flate: [], fonts: [],
  charToGlyph: 0, charsToGlyphs: 0, charstrings: 0, decodeCharString: 0 };
const ctx = vm.createContext({ console, Math, Date, __count });
vm.runInContext(src, ctx, { filename: 'pdfjs.js' });
const t0 = Date.now();
vm.runInContext(`for (let i = 0; i < ${iterations}; i++) new Benchmark().runIteration();`, ctx);
const ms = Date.now() - t0;

const logs = vm.runInContext('canvas_logs', ctx);
const log = logs[0];
// Same as tearDownPdfJS: hash of join(" ") with the JS int32 hash.
function hash(s) {
  let h = 0;
  for (let i = 0; 4 * i - 3 < s.length; i += 4)
    for (let j = 0; j < 4 && i + j < s.length; ++j)
      h = (h + s.charCodeAt(i + j) << (8 * j)) | 0;
  return h;
}
const joined = log.join(' ');
// Full-string FNV-1a over the UTF-16 code units of join(" ") (the C check).
let fullHash = 0x811c9dc5;
for (let i = 0; i < joined.length; i++) fullHash = Math.imul(fullHash ^ joined.charCodeAt(i), 16777619) >>> 0;
const kinds = {};
for (const x of log) if (typeof x === 'string' && x !== '\n') kinds[x] = (kinds[x] || 0) + 1;

const fontInfo = __count.fonts.map(f => {
  let sum = 0; const d = f.data || [];
  for (let i = 0; i < d.length; i++) sum = (sum * 31 + d[i]) >>> 0;
  return { name: f.loadedName, bytes: d.length, checksum: sum, glyphs: f.charstrings ? f.charstrings.length : undefined };
});
const flateInfo = __count.flate.map(s => s.bufferLength);

console.log(JSON.stringify({
  iterations, ms,
  canvasLogs: logs.length, logLength: log.length, logHash: hash(joined), logFullHash: fullHash,
  expected: { length: 36788, hash: 939524096 },
  logItemsByOp: kinds,
  opsByName: __count.ops,
  parserGetObj: __count.parserGetObj,
  streamGetChar: __count.streamGetChar,
  flateStreams: flateInfo,
  fonts: fontInfo,
  charToGlyph: __count.charToGlyph, charsToGlyphs: __count.charsToGlyphs,
  charstrings: __count.charstrings, decodeCharString: __count.decodeCharString,
}, null, 1));
