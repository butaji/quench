// Validation twin for the typescript-lib analogue (typescript-lib.c).
//
// Loads the same TypeScript version as TypeScript/dist/bundle.js (5.9.2) from an npm package
// directory and runs the same work as TypeScript/src/test.mjs compileTest over the immer-tiny
// project (118 files). Prints per-file and program-level counts that the C analogue must match.
//
// usage: node typescript-lib-validate.mjs [jetstream-root] [typescript-package-dir]
//   typescript-package-dir defaults to $TS_PACKAGE_DIR, else /tmp/claude-0/scratch-typescript-lib/ts592/package
//   (get it with: npm pack typescript@5.9.2 && tar xzf typescript-5.9.2.tgz -C DIR).
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";

const root = process.argv[2] ?? "../jetstream";
const pkg = process.argv[3] ?? process.env.TS_PACKAGE_DIR ?? "/tmp/claude-0/scratch-typescript-lib/ts592/package";
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
  diagnostics: lines.slice(0, 20),
  resultFilesCount: Object.keys(host.outFileData).length,
  ms: { createProgram: +tCreate.toFixed(1), emit: +tEmit.toFixed(1), preEmitDiagnostics: +tDiag.toFixed(1) },
};
console.log(JSON.stringify(out, null, 2));
if (process.env.TS_PER_FILE) for (const r of perFile) console.log(r.join("\t"));
