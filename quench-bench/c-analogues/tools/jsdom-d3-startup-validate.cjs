// Node reference for the jsdom-d3-startup C analogue (quench-bench/c-analogues/jsdom-d3-startup.c).
//
// Usage: node jsdom-d3-startup-validate.cjs [jetstream-root]
//
// Runs the row's bundle exactly as StartupBenchmark does (the cache-busting comment replaced per
// iteration pair, eval per iteration, then D3Test.runTest on the CSV string and the parsed TopoJSON)
// for one iteration, and prints the values the C analogue checks: the output length in UTF-16 units,
// quickHash of the output, element and path-command counts, and facts about the CSV columns that the
// autoType conversion depends on. Output is JSON.
const fs = require('fs');
const vm = require('vm');
const path = require('path');

const root = process.argv[2] || path.join(__dirname, '../../../../quench/quench-bench/jetstream');
const row = path.join(root, 'jsdom-d3-startup');
const CACHE_BUST = '\n/*ThouShaltNotCache*/\n';

function quickHash(str) {
  let hash = 5381;
  let i = str.length;
  while (i > 0) {
    hash = (hash * 33) ^ (str.charCodeAt(i) | 0);
    i -= 919;
  }
  return hash | 0;
}

// Same copy as StartupBenchmark.createIterationSourceCode for iteration 0 (cacheId 0).
const bundle = fs.readFileSync(path.join(row, 'dist/bundle.min.js'), 'utf8');
const copy = bundle.split(CACHE_BUST).join('/*0*/');
const cacheCount = bundle.split(CACHE_BUST).length - 1;
vm.runInThisContext(copy, { filename: 'bundle.min.js' });

const us = JSON.parse(fs.readFileSync(path.join(row, 'data/counties-albers-10m.json'), 'utf8'));
const csv = fs.readFileSync(path.join(row, 'data/airports.csv'), 'utf8');
const html = globalThis.D3Test.runTest(csv, us);

// Element counts by tag name (the serializer writes plain <tag ...> and </tag>).
const tags = {};
for (const m of html.matchAll(/<([A-Za-z][A-Za-z0-9]*)[ >]/g)) tags[m[1]] = (tags[m[1]] || 0) + 1;
const dValues = [...html.matchAll(/ d="([^"]*)"/g)].map((m) => m[1]);
let mCmds = 0, lCmds = 0, zCmds = 0;
for (const d of dValues) {
  for (const c of d) {
    if (c === 'M') mCmds++;
    else if (c === 'L') lCmds++;
    else if (c === 'Z') zCmds++;
  }
}
const pathElems = (html.match(/<path[ >]/g) || []).length;
const pathNoD = (html.match(/<path>/g) || []).length;

// CSV facts: parse with RFC 4180 quoting and classify the columns autoType looks at.
function parseCsv(text) {
  const rows = [];
  let row = [], field = '', i = 0, quoted = false;
  while (i < text.length) {
    const c = text[i];
    if (quoted) {
      if (c === '"') {
        if (text[i + 1] === '"') { field += '"'; i++; } else quoted = false;
      } else field += c;
    } else if (c === '"') quoted = true;
    else if (c === ',') { row.push(field); field = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++;
      row.push(field); rows.push(row); row = []; field = '';
    } else field += c;
    i++;
  }
  if (field !== '' || row.length) { row.push(field); rows.push(row); }
  return rows;
}
const rows = parseCsv(csv);
const header = rows[0].map((h) => h.trim());
const col = (name) => header.indexOf(name);
const classify = (raw) => {
  const v = raw.trim();
  if (!v) return 'null';
  if (v === 'true' || v === 'false') return 'bool';
  if (v === 'NaN') return 'NaN';
  if (!isNaN(+v)) return 'number';
  if (/^([-+]\d{2})?\d{4}(-\d{2}(-\d{2})?)?(T\d{2}:\d{2}(:\d{2}(\.\d{3})?)?(Z|[-+]\d{2}:\d{2})?)?$/.test(v)) return 'date';
  return 'string';
};
const facts = {};
for (const name of ['name', 'latitude', 'longitude', 'iata_code', 'municipality']) {
  const c = col(name);
  const kinds = {};
  for (const r of rows.slice(1)) {
    if (r.length !== header.length) continue;
    const k = classify(r[c]);
    kinds[k] = (kinds[k] || 0) + 1;
  }
  facts[name] = kinds;
}
const nonAscii = [...csv].filter((ch) => ch.charCodeAt(0) > 127).length;
const titleSample = html.match(/<title>[^<]*<\/title>/)[0];

const out = {
  cacheCommentCount: cacheCount,
  airportRows: rows.length - 1,
  csvQuotedLines: csv.split('\n').filter((l) => l.includes('"')).length,
  csvNonAsciiChars: nonAscii,
  htmlUtf16Length: html.length,
  htmlBytes: Buffer.byteLength(html),
  htmlQuickHash: quickHash(html),
  tagCounts: tags,
  pathElements: pathElems,
  pathWithoutD: pathNoD,
  dAttributes: dValues.length,
  pathCommands: { M: mCmds, L: lCmds, Z: zCmds },
  titleSample: titleSample.slice(0, 80),
  autoTypeFacts: facts,
};
console.log(JSON.stringify(out, null, 2));
