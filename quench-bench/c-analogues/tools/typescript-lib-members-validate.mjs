// Member validation twin for the typescript-lib analogue (typescript-lib.c, --members).
//
// For every class and interface declared in the immer-tiny program (program order, one line per merged
// symbol) prints: file, name, number of value members and their escaped names, sorted, with inherited
// members included (checker.getPropertiesOfType of the declared type). Late-bound names __@name@id have
// their id replaced by '*', as the C side prints them. The C analogue must print the same lines:
//   typescript-lib <iterations> <jetstream-root> --members
//
// usage: node typescript-lib-members-validate.mjs [jetstream-root] [typescript-package-dir]
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const root = args[0] ?? "../jetstream";
const pkg = args[1] ?? process.env.TS_PACKAGE_DIR ?? "/tmp/claude-0/scratch-typescript-lib/ts592/package";
const require = createRequire(import.meta.url);
const ts = require(path.join(pkg, "lib/typescript.js"));

const dir = path.join(root, "TypeScript/src/gen/immer-tiny");
const tsConfig = JSON.parse(fs.readFileSync(path.join(dir, "actually-a-type-script-config-please-dont-firewall-me.json"), "utf8"));
const srcFileData = JSON.parse(fs.readFileSync(path.join(dir, "files.json"), "utf8"));
const REPO_ROOT = "/User/test/JetStream/";

// Same host as typescript-lib-validate.mjs (TypeScript/src/test.mjs).
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
const program = ts.createProgram(Object.keys(srcFileData), options, host);
const checker = program.getTypeChecker();

const normalize = (name) => name.replace(/^__@(.*)@\d+$/, "__@$1@*");
const seen = new Set();   // one line per merged symbol, in the file of its first declaration
for (const sf of program.getSourceFiles()) {
  const visit = (n) => {
    if ((ts.isClassDeclaration(n) || ts.isInterfaceDeclaration(n)) && n.name) {
      const sym = checker.getSymbolAtLocation(n.name);
      if (sym && !seen.has(sym)) {
        seen.add(sym);
        const props = checker.getPropertiesOfType(checker.getDeclaredTypeOfSymbol(sym));
        const names = props.map((p) => normalize(p.escapedName)).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
        console.log(`${sf.fileName} ${n.name.text} ${names.length}${names.map((x) => " " + x).join("")}`);
      }
    }
    ts.forEachChild(n, visit);
  };
  visit(sf);
}
