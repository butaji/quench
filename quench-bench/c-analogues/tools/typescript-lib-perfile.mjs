// Per-file reference counts for the typescript-lib analogue (typescript-lib.c).
//
// Prints one TSV line per source file of the immer-tiny program:
//   fileName <TAB> scriptKind <TAB> nodeCount(ParseAll) <TAB> identifierCount(ParseAll)
//                           <TAB> nodeCount(ParseNone) <TAB> identifierCount(ParseNone)
// ParseAll is TypeScript's default (JSDoc parsed); ParseNone skips JSDoc (parser-only reference).
// Also writes the immer-tiny source files under <out-dir>/files/ when a third argument is given,
// so the C port can be compared file by file without re-reading files.json.
//
// usage: node typescript-lib-perfile.mjs [jetstream-root] [typescript-package-dir] [out-dir]
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const root = process.argv[2] ?? "../jetstream";
const pkg = process.argv[3] ?? process.env.TS_PACKAGE_DIR ?? "/tmp/claude-0/scratch-typescript-lib/ts592/package";
const outDir = process.argv[4];
const require = createRequire(import.meta.url);
const ts = require(path.join(pkg, "lib/typescript.js"));

const dir = path.join(root, "TypeScript/src/gen/immer-tiny");
const tsConfig = JSON.parse(fs.readFileSync(path.join(dir, "actually-a-type-script-config-please-dont-firewall-me.json"), "utf8"));
const srcFileData = JSON.parse(fs.readFileSync(path.join(dir, "files.json"), "utf8"));
const REPO_ROOT = "/User/test/JetStream/";
const options = ts.convertCompilerOptionsFromJson(tsConfig.compilerOptions, REPO_ROOT).options;
const target = ts.getEmitScriptTarget(options);

for (const name of Object.keys(srcFileData)) {
  const text = srcFileData[name];
  const all = ts.createSourceFile(name, text, { languageVersion: target, jsDocParsingMode: 0 });
  const none = ts.createSourceFile(name, text, { languageVersion: target, jsDocParsingMode: 1 });
  const kind = all.scriptKind ?? "";
  console.log([name, kind, all.nodeCount, all.identifierCount, none.nodeCount, none.identifierCount].join("\t"));
  if (outDir) {
    const dst = path.join(outDir, "files", name);
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.writeFileSync(dst, text);
  }
}
