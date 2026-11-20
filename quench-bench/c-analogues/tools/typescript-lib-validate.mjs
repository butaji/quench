// Validation twin for the typescript-lib analogue (typescript-lib.c).
//
// Loads the same TypeScript version as TypeScript/dist/bundle.js (5.9.2) from an npm package
// directory and runs the same work as TypeScript/src/test.mjs compileTest over the immer-tiny
// project (118 files). Prints per-file and program-level counts that the C analogue must match.
//
// usage: node typescript-lib-validate.mjs [jetstream-root] [typescript-package-dir] [--header OUT.h]
//   typescript-package-dir defaults to $TS_PACKAGE_DIR, else /tmp/claude-0/scratch-typescript-lib/ts592/package
//   (get it with: npm pack typescript@5.9.2 && tar xzf typescript-5.9.2.tgz -C DIR).
//   --header writes support/ts-checker-expected.h (program file order, the diagnostics of
//   getPreEmitDiagnostics with file, line, column and code, and the counts after checker init).
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";

const headerIdx = process.argv.indexOf("--header");
const headerOut = headerIdx > 0 ? process.argv[headerIdx + 1] : null;
const args = process.argv.slice(2).filter((a, i, all) => a !== "--header" && all[i - 1] !== "--header");
const root = args[0] ?? "../jetstream";
const pkg = args[1] ?? process.env.TS_PACKAGE_DIR ?? "/tmp/claude-0/scratch-typescript-lib/ts592/package";
const require = createRequire(import.meta.url);
const ts = require(path.join(pkg, "lib/typescript.js"));

const dir = path.join(root, "TypeScript/src/gen/immer-tiny");
const tsConfig = JSON.parse(fs.readFileSync(path.join(dir, "actually-a-type-script-config-please-dont-firewall-me.json"), "utf8"));
const srcFileData = JSON.parse(fs.readFileSync(path.join(dir, "files.json"), "utf8"));
const REPO_ROOT = "/User/test/JetStream/";

// Same host as TypeScript/src/test.mjs.
class CompilerHost {
  constructor(options, data) { this.options = options; this.srcFileData = data; this.outFileData = Object.create(null); }
  getSourceFile(fileName, languageVersion) { return ts.createSourceFile(fileName, this.readFile(fileName), languageVersion); }
  resolveModuleNames(moduleNames, containingFile) {
    return moduleNames.map((m) => {
      const r = ts.resolveModuleName(m, containingFile, this.options, this);
      return r.resolvedModule ? r.resolvedModule : undefined;
    });
  }
  getDefaultLibFileName() { return "lib.d.ts"; }
  getCurrentDirectory() { return ""; }
  getCanonicalFileName(f) { return f.toLowerCase(); }
  useCaseSensitiveFileNames() { return true; }
  getNewLine() { return "\n"; }
  fileExists(p) { return p in this.srcFileData; }
  readFile(p) {
    const c = this.srcFileData[p.toLowerCase()];
    if (c === undefined) throw new Error(`"${p}" does not exist.`);
    return c;
  }
  writeFile(name, data) { this.outFileData[name] = data; }
}

const options = ts.convertCompilerOptionsFromJson(tsConfig.compilerOptions, REPO_ROOT).options;
options.lib = [...(options.lib || []), "dom"];
const host = new CompilerHost(options, srcFileData);

// Count nodes the way ts.forEachChild sees them (SourceFile's own children plus all descendants).
function walkCount(sf) {
  let nodes = 0, idents = 0;
  const visit = (n) => {
    nodes++;
    if (n.kind === ts.SyntaxKind.Identifier) idents++;
    ts.forEachChild(n, visit);
  };
  ts.forEachChild(sf, visit);
  return { nodes: nodes + 1, idents };
}

const names = Object.keys(srcFileData);
let t0 = performance.now();
const program = ts.createProgram(names, options, host);
const tCreate = performance.now() - t0;
// checker initialization alone (createTypeChecker + initializeTypeChecker: merges globals)
const checker = program.getTypeChecker();
const afterInit = { symbolCount: program.getSymbolCount(), typeCount: program.getTypeCount() };
t0 = performance.now();
const emitResult = program.emit();
const tEmit = performance.now() - t0;
t0 = performance.now();
const diags = ts.getPreEmitDiagnostics(program).concat(emitResult.diagnostics);
const tDiag = performance.now() - t0;

const perFile = [];
let sumNodes = 0, sumIdents = 0, walkNodes = 0, walkIdents = 0, mismatch = 0;
for (const sf of program.getSourceFiles()) {
  const w = walkCount(sf);
  // sourceFile.nodeCount / identifierCount are the parser's own counters.
  perFile.push([sf.fileName, sf.nodeCount, sf.identifierCount, w.nodes, w.idents]);
  sumNodes += sf.nodeCount; sumIdents += sf.identifierCount;
  walkNodes += w.nodes; walkIdents += w.idents;
  if (sf.nodeCount !== w.nodes || sf.identifierCount !== w.idents) mismatch++;
}
const lines = diags.map((d) => {
  const msg = ts.flattenDiagnosticMessageText(d.messageText, "\n");
  if (d.file) {
    const { line, character } = ts.getLineAndCharacterOfPosition(d.file, d.start);
    return `${d.file.fileName} (${line + 1},${character + 1}): ${msg}`;
  }
  return msg;
});
// every diagnostic with its position and code, in getPreEmitDiagnostics order
const diagList = diags.map((d) => {
  if (!d.file) return { file: null, line: 0, column: 0, code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, "\n") };
  const { line, character } = ts.getLineAndCharacterOfPosition(d.file, d.start);
  return { file: d.file.fileName, line: line + 1, column: character + 1, code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, "\n") };
});
if (headerOut) {
  const esc = (s) => JSON.stringify(s);
  const h = [];
  h.push("/* Generated by tools/typescript-lib-validate.mjs --header from TypeScript " + ts.version + ". Do not edit. */");
  h.push("#ifndef TS_CHECKER_EXPECTED_H", "#define TS_CHECKER_EXPECTED_H", "");
  h.push(`#define EXPECTED_PROGRAM_FILES ${program.getSourceFiles().length}`);
  h.push("static const char *expectedProgramOrder[EXPECTED_PROGRAM_FILES] = {");
  for (const sf of program.getSourceFiles()) h.push(`  ${esc(sf.fileName)},`);
  h.push("};", "");
  h.push(`#define EXPECTED_SYMBOLS_AFTER_INIT ${afterInit.symbolCount}`);
  h.push(`#define EXPECTED_TYPES_AFTER_INIT ${afterInit.typeCount}`);
  h.push(`#define EXPECTED_SYMBOLS ${program.getSymbolCount()}`);
  h.push(`#define EXPECTED_TYPES ${program.getTypeCount()}`);
  h.push(`#define EXPECTED_INSTANTIATIONS ${program.getInstantiationCount()}`);
  h.push(`#define EXPECTED_REL_ASSIGNABLE ${program.getRelationCacheSizes().assignable}`);
  h.push(`#define EXPECTED_REL_IDENTITY ${program.getRelationCacheSizes().identity}`);
  h.push(`#define EXPECTED_REL_SUBTYPE ${program.getRelationCacheSizes().subtype}`);
  h.push(`#define EXPECTED_REL_STRICT_SUBTYPE ${program.getRelationCacheSizes().strictSubtype}`);
  h.push("", "typedef struct { int code; const char *file; int line; int column; } ExpectedDiag;");
  h.push(`#define EXPECTED_DIAG_COUNT ${diagList.length}`);
  h.push("static const ExpectedDiag expectedDiags[EXPECTED_DIAG_COUNT] = {");
  for (const d of diagList) h.push(`  { ${d.code}, ${d.file ? esc(d.file) : "NULL"}, ${d.line}, ${d.column} }, /* ${d.message.split("\n")[0].slice(0, 90).replace(/\*\//g, "*_/")} */`);
  h.push("};", "", "#endif", "");
  fs.writeFileSync(headerOut, h.join("\n"));
}

const out = {
  typescript: ts.version,
  files: program.getSourceFiles().length,
  sumNodeCount: sumNodes,
  sumIdentifierCount: sumIdents,
  walkNodeCount: walkNodes,
  walkIdentifierCount: walkIdents,
  filesWhereParserCountDiffersFromWalk: mismatch,
  programNodeCount: program.getNodeCount(),
  programIdentifierCount: program.getIdentifierCount(),
  symbolCount: program.getSymbolCount(),
  typeCount: program.getTypeCount(),
  instantiationCount: program.getInstantiationCount(),
  relationCacheSizes: program.getRelationCacheSizes(),
  diagnosticCount: lines.length,
  diagnostics: diagList,
  afterCheckerInit: afterInit,
  resultFilesCount: Object.keys(host.outFileData).length,
  ms: { createProgram: +tCreate.toFixed(1), emit: +tEmit.toFixed(1), preEmitDiagnostics: +tDiag.toFixed(1) },
};
console.log(JSON.stringify(out, null, 2));
if (process.env.TS_PER_FILE) for (const r of perFile) console.log(r.join("\t"));
