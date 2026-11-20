// Node-side work counts for typescript-lib.c: the reference that the C port is compared with, per function and
// per interface. It writes an instrumented copy of TypeScript 5.9.2's lib/typescript.js, then checks the
// immer-tiny program with it, file by file.
//
//   - per function (exclusive): types created, instantiations, and new entries of the assignable, identity,
//     subtype and strictSubtype relation caches. The bucket key is "<leaf function> <- <caller> @ <top check>",
//     where <top check> is the checkSourceElement or checkSourceFile function that started the work.
//   - per interface: the new assignable pairs, types and instantiations added while checkInterfaceDeclaration
//     runs for that interface (the C port prints the same numbers under --iface-counts).
//   - TRACE_IFACE=<name>: the assignable pairs (source => target, by typeToString) that the named interface adds,
//     written to <out-dir>/trace-<name>.txt.
//
// usage: node tools/typescript-lib-instr.cjs <typescript-package-dir> [jetstream-root] [out-dir]
//   typescript-package-dir: the unpacked typescript@5.9.2 npm package (lib/typescript.js is instrumented)
//   jetstream-root: default ../jetstream; out-dir: default ./typescript-lib-instr-out
// Writes out-dir/typescript-instr.js, out-dir/per-file.json and the per-interface table to stdout.
"use strict";
const fs = require("fs");
const path = require("path");
const { createRequire } = require("node:module");

const pkg = process.argv[2];
if (!pkg) {
  console.error("usage: node tools/typescript-lib-instr.cjs <typescript-package-dir> [jetstream-root] [out-dir]");
  process.exit(2);
}
const root = process.argv[3] ?? "../jetstream";
const outDir = process.argv[4] ?? "typescript-lib-instr-out";
const traceIface = process.env.TRACE_IFACE || "";
fs.mkdirSync(outDir, { recursive: true });

// Functions whose work is attributed to their caller: the type constructors and the common helpers.
const HELPERS = new Set(["createType", "createTypeWithSymbol", "createObjectType", "createLiteralType", "createTypeParameter",
  "createIndexedAccessType", "createTemplateLiteralType", "createIntersectionType", "createUniqueESSymbolType",
  "getUnionTypeFromSortedList", "getUnionTypeWorker", "getUnionType", "instantiateTypeWithAlias", "instantiateType",
  "instantiateTypeWorker", "instantiateList", "instantiateIndexInfo", "instantiateTypePredicate", "instantiateMappedType",
  "getIndexedAccessType", "getIndexedAccessTypeOrUndefined", "getTypeFromTypeNode", "getTypeFromTypeNodeWorker", "filterType",
  "getIntersectionType", "getTemplateLiteralType", "getTypeOfSymbol", "getTypeOfSymbolWorker", "getDeclaredTypeOfSymbol",
  "resolveStructuredTypeMembers", "resolveObjectTypeMembers", "getTypeOfInstantiatedSymbol", "getReturnTypeOfSignature",
  "getSignaturesOfType", "isTypeRelatedTo", "isTypeAssignableTo", "getWidenedType", "getWidenedTypeWithContext",
  "getRegularTypeOfLiteralType", "getTypeWithThisArgument", "getTypeOfPropertyOfType", "getPropertiesOfType", "getMappedType",
  "resolveAnonymousTypeMembers", "getBaseTypes", "getTypeArguments", "createTypeReference", "getTypeReferenceArity",
  "createInstantiatedSymbolTable", "createSymbol", "resolveTypeReferenceMembers", "getApparentType"]);

// ---- the instrumented copy ----
const src = path.join(pkg, "lib/typescript.js");
const lines = fs.readFileSync(src, "utf8").split("\n");
const findLine = (pred, from = 0) => lines.findIndex((l, i) => i >= from && pred(l));
const start = findLine((l) => l.startsWith("function createTypeChecker(host)"));
const closeIdx = findLine((l) => l === "}", start + 1);
const names = new Set();
for (let i = start; i < closeIdx; i++) {
  const m = /^  function (\w+)\(/.exec(lines[i]);
  if (m) names.add(m[1]);
}
// Relation caches become a Map subclass that records each new assignable entry with the pair on top of the stack.
const relMaps = [];
for (let i = start; i < closeIdx; i++) {
  const m = /^  var (\w+Relation) = \/\* @__PURE__ \*\/ new Map\(\);$/.exec(lines[i]);
  if (m) {
    relMaps.push(m[1]);
    lines[i] = `  var ${m[1]} = new __TSI_RelMap(${JSON.stringify(m[1])});`;
  }
}
if (!relMaps.includes("assignableRelation") || !relMaps.includes("identityRelation")) throw new Error("relation maps not found");
const idLine = findLine((l) => l === "    result.id = typeCount;", start);
if (idLine < 0 || idLine > closeIdx) throw new Error("createType anchor not found");
lines[idLine] = "    result.id = typeCount;\n    if (globalThis.__TSI) globalThis.__TSI.byId[typeCount] = result;";

const prelude = [
  "  globalThis.__TSI = { byId: [], pairStack: [], pairs: [], iface: null, traceOn: false, ifaceLog: [], buckets: new Map(), stack: [], last: null };",
  "  const __TSI_snap = () => [typeCount, totalInstantiationCount, assignableRelation.size, identityRelation.size, subtypeRelation.size, strictSubtypeRelation.size];",
  "  const __TSI_top = (st) => { for (let q = st.length - 1; q >= 0; q--) if (st[q] === 'checkSourceElementWorker' || st[q] === 'checkSourceFileWorker') return st[q + 1] || st[q]; return '<root>'; };",
  "  const __TSI_helpers = " + JSON.stringify([...HELPERS]) + ";",
  "  const __TSI_accrue = (leaf) => {",
  "    const I = globalThis.__TSI; const st = I.stack; const top = __TSI_top(st);",
  "    let key = leaf, found = 0;",
  "    for (let q = st.length - 1; q >= 0 && found < 2; q--) {",
  "      if (st[q] === 'checkSourceElementWorker' || st[q] === 'checkSourceFileWorker') break;",
  "      if (__TSI_helpers.includes(st[q])) continue;",
  "      key += ' <- ' + st[q]; found++;",
  "    }",
  "    if (found === 0) key += ' <- ' + top;",
  "    const now = __TSI_snap();",
  "    if (!I.last) { I.last = now; return; }",
  "    const name = key + ' @ ' + top;",
  "    const b = I.buckets.get(name) || [0, 0, 0, 0, 0, 0];",
  "    now.forEach((v, k) => { b[k] += v - I.last[k]; });",
  "    I.buckets.set(name, b); I.last = now;",
  "  };",
  "  const __TSI_wrap = (name, f) => function (...args) {",
  "    const I = globalThis.__TSI; __TSI_accrue(I.stack.length ? I.stack[I.stack.length - 1] : '<root>'); I.stack.push(name);",
  "    try { return f.apply(this, args); } finally { __TSI_accrue(name); I.stack.pop(); }",
  "  };",
  "  globalThis.__TSI.flush = () => { const I = globalThis.__TSI; __TSI_accrue(I.stack.length ? I.stack[I.stack.length - 1] : '<root>'); };",
  "  globalThis.__TSI.begin = () => { const I = globalThis.__TSI; I.last = __TSI_snap(); I.buckets = new Map(); };",
  "  globalThis.__TSI.snap = __TSI_snap;",
  "  { const __ci = checkInterfaceDeclaration; checkInterfaceDeclaration = function (node) {",
  "    const I = globalThis.__TSI; const b = __TSI_snap(); const outer = I.iface;",
  "    const name = node.name ? node.name.escapedText : '?';",
  "    I.iface = name;",
  "    const r = __ci.apply(this, arguments);",
  "    I.iface = outer; const a = __TSI_snap();",
  "    I.ifaceLog.push([getSourceFileOfNode(node).fileName, name, a[2] - b[2], a[0] - b[0], a[1] - b[1]]);",
  "    return r; }; }",
  ...[...names].map((n) => `  ${n} = __TSI_wrap(${JSON.stringify(n)}, ${n});`),
];
// The relation map class comes first in createTypeChecker (the maps are created before initializeTypeChecker).
const relClass = [
  "  class __TSI_RelMap extends Map {",
  "    constructor(name) { super(); this.relName = name; }",
  "    set(key, value) {",
  "      const I = globalThis.__TSI;",
  "      if (I && I.traceOn && I.iface && this.relName === 'assignableRelation' && !this.has(key)) I.pairs.push({ iface: I.iface, key, stack: I.stack.slice(-10).join(' > ') });",
  "      return super.set(key, value);",
  "    }",
  "  }",
];
lines.splice(start + 1, 0, ...relClass);
// The accounting goes before initializeTypeChecker(), which runs the checker's global setup.
const initIdx = findLine((l) => l === "  initializeTypeChecker();", start);
lines.splice(initIdx, 0, ...prelude);
const instrPath = path.join(outDir, "typescript-instr.js");
fs.writeFileSync(instrPath, lines.join("\n"));
console.error(`wrapped ${names.size} functions into ${instrPath}`);

// ---- the per-file run (node 5.9.2 checks each source file of the immer-tiny program) ----
const requireFrom = createRequire(__filename);
const ts = requireFrom(path.resolve(instrPath));
const dir = path.join(root, "TypeScript/src/gen/immer-tiny");
const tsConfig = JSON.parse(fs.readFileSync(path.join(dir, "actually-a-type-script-config-please-dont-firewall-me.json"), "utf8"));
const srcFileData = JSON.parse(fs.readFileSync(path.join(dir, "files.json"), "utf8"));
const REPO_ROOT = "/User/test/JetStream/";
const options = ts.convertCompilerOptionsFromJson(tsConfig.compilerOptions, REPO_ROOT).options;
options.lib = [...(options.lib || []), "lib.dom.d.ts"];
const host = {
  getSourceFile: (f, lv) => ts.createSourceFile(f, host.readFile(f), lv),
  resolveModuleNames: (mn, cf) => mn.map((m) => ts.resolveModuleName(m, cf, options, host).resolvedModule || undefined),
  getDefaultLibFileName: () => "lib.d.ts", getCurrentDirectory: () => "", getCanonicalFileName: (f) => f.toLowerCase(),
  useCaseSensitiveFileNames: () => true, getNewLine: () => "\n",
  // lib files that files.json does not carry come from the package's lib directory
  fileExists: (p) => p in srcFileData || fs.existsSync(path.join(pkg, "lib", path.basename(p))),
  readFile: (p) => {
    const c = srcFileData[p.toLowerCase()];
    if (c !== undefined) return c;
    const libPath = path.join(pkg, "lib", path.basename(p));
    if (fs.existsSync(libPath)) return fs.readFileSync(libPath, "utf8");
    throw new Error(p);
  },
  writeFile: () => {},
};
const program = ts.createProgram(Object.keys(srcFileData), options, host);
const checker = program.getTypeChecker();
const I = globalThis.__TSI;   // set by createTypeChecker
const labels = ["types", "inst", "assign", "identity", "subtype", "strictSub"];
const perFile = {};
const total = [0, 0, 0, 0, 0, 0];
const traced = [];
I.traceOn = traceIface !== "";
for (const sf of program.getSourceFiles()) {
  I.begin();
  I.stack = [];
  I.iface = null;
  program.getSemanticDiagnostics(sf);
  I.flush();
  const per = {};
  for (const [name, b] of I.buckets) { per[name] = b; for (let k = 0; k < 6; k++) total[k] += b[k]; }
  perFile[sf.fileName] = per;
}
fs.writeFileSync(path.join(outDir, "per-file.json"), JSON.stringify({ labels, files: perFile, total }, null, 1));
console.log("total " + labels.map((l, k) => `${l}=${total[k]}`).join(" "));

// Per interface: the sum over its checkInterfaceDeclaration calls (the calls include nested work).
const agg = new Map();
for (const [file, name, assign, types, inst] of I.ifaceLog) {
  const key = name + " (" + path.basename(file) + ")";
  const a = agg.get(key) || [0, 0, 0];
  a[0] += assign; a[1] += types; a[2] += inst;
  agg.set(key, a);
}
const rows = [...agg.entries()].sort((x, y) => y[1][0] - x[1][0]);
fs.writeFileSync(path.join(outDir, "iface-counts.tsv"), "interface\tassignable\ttypes\tinstantiations\n" +
  rows.map(([k, v]) => `${k}\t${v.join("\t")}`).join("\n") + "\n");
console.log(`checkInterfaceDeclaration calls ${I.ifaceLog.length}; table in ${path.join(outDir, "iface-counts.tsv")}`);

if (traceIface) {
  const out = [];
  // keys of plain pairs are "source,target" type ids (optionally "*" or ":flags"); reference keys are not decoded
  for (const { iface, key, stack: pairStack } of I.pairs) {
    if (iface !== traceIface) continue;
    const m = /^\*?(\d+),(\d+)/.exec(key);
    if (!m) { out.push(`(reference key) ${key}`); continue; }
    const s = I.byId[m[1]], t = I.byId[m[2]];
    const stack = process.env.TRACE_STACK ? `   [${pairStack}]` : "";
    out.push(`${checker.typeToString(s).slice(0, 120)}  =>  ${checker.typeToString(t).slice(0, 120)}${stack}`);
  }
  const file = path.join(outDir, `trace-${traceIface}.txt`);
  fs.writeFileSync(file, out.join("\n") + "\n");
  console.log(`traced ${out.length} assignable pairs of ${traceIface} into ${file}`);
}
